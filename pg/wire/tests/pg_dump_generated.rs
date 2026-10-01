#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use proptest::prelude::*;
use proptest::strategy::Union;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use turso_core::MemoryIO;
use turso_pg_head::Head;
use turso_pg_head_wire::serve;

mod common;

use common::pgwire::{BackendEvent, ConnParams, PgConn};

const ROLES: [&str; 3] = ["alice", "Bob Q", "carol"];
const TABLE_NAMES: [&str; 5] = ["notes", "Mixed Case", "a.b", "select", "t_pkey"];
const COLUMN_NAMES: [&str; 5] = ["id", "owner", "Body", "x.y", "order"];
const TABLE_PRIVILEGES: [&str; 9] = [
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "TRUNCATE",
    "REFERENCES",
    "TRIGGER",
    "MAINTAIN",
    "ALL",
];

/// Bounds on the policy expression grammar below: how many `AND`/`OR`/`NOT`
/// levels a `USING` expression nests (`boolean_expr`), and how many
/// `CAST`/`CASE` wrappers an operand nests (`operand`). Kept small so a
/// generated schema stays cheap to apply twice (head, PostgreSQL) and dump,
/// and so a failure's expression is short enough to read as a reproducer.
const BOOLEAN_DEPTH: u32 = 2;
const OPERAND_DEPTH: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Type {
    Integer,
    BigInt,
    Text,
}

#[derive(Debug, Clone)]
struct Column {
    name: &'static str,
    ty: Type,
    not_null: bool,
}

#[derive(Debug, Clone)]
struct Table {
    name: &'static str,
    owner: Option<usize>,
    columns: Vec<Column>,
    primary_key: Option<usize>,
    grants: Vec<(Vec<&'static str>, Option<usize>)>,
    enable: bool,
    force: bool,
    policies: Vec<(Option<usize>, Expr)>,
}

/// A policy's `USING` expression: PostgreSQL 18 requires this to type as
/// boolean (`Position::Policy`'s own `require_boolean` rule), so every
/// variant here already is one; nesting happens through `Operand::Case` and
/// `Operand::Cast` instead, the same as PostgreSQL's own boolean-valued
/// expression grammar.
#[derive(Debug, Clone)]
enum Expr {
    Compare(CompareOp, Operand, Operand),
    IsNull(Operand, bool),
    IsDistinct(Operand, Operand, bool),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    fn sql(self) -> &'static str {
        match self {
            CompareOp::Eq => "=",
            CompareOp::Ne => "<>",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }
}

fn compare_op() -> impl Strategy<Value = CompareOp> {
    prop_oneof![
        Just(CompareOp::Eq),
        Just(CompareOp::Ne),
        Just(CompareOp::Lt),
        Just(CompareOp::Le),
        Just(CompareOp::Gt),
        Just(CompareOp::Ge),
    ]
}

/// One of the three declarable column types (`CREATE TABLE`'s own admitted
/// set), the only cast targets this grammar uses: every cast PostgreSQL 18
/// itself accepts between them (`integer`/`bigint` widen or narrow, either
/// casts to and from `text`), so a mismatch here is always a head bug, not
/// a grammar bug.
#[derive(Debug, Clone, Copy)]
enum CastType {
    Integer,
    BigInt,
    Text,
}

impl CastType {
    fn sql(self) -> &'static str {
        match self {
            CastType::Integer => "integer",
            CastType::BigInt => "bigint",
            CastType::Text => "text",
        }
    }
}

fn cast_type_of(ty: Type) -> CastType {
    match ty {
        Type::Integer => CastType::Integer,
        Type::BigInt => CastType::BigInt,
        Type::Text => CastType::Text,
    }
}

#[derive(Debug, Clone)]
enum Operand {
    Column(&'static str),
    Integer(i64),
    QuotedInteger(i64),
    Text(String),
    Null,
    CurrentUser,
    CurrentSetting(String),
    Cast(Box<Operand>, CastType),
    /// A searched `CASE`: PostgreSQL's `pg_get_expr` reproduces this
    /// spelling exactly (`render/definition.rs`'s `Typed::Case` arm), so
    /// this always exercises real rendering, not only refusal. Always
    /// carries an `ELSE`, so its arms never need a `NULL`-defaulting
    /// fourth type to unify against.
    Case(Vec<(Expr, Operand)>, Box<Operand>),
}

#[derive(Debug, Clone)]
struct Schema {
    logins: [bool; 3],
    tables: Vec<Table>,
}

#[derive(Debug, Default)]
struct Outcomes {
    both_accept: usize,
    /// PostgreSQL and the head refuse the same generated policy with the
    /// same SQLSTATE (an ill-typed expression), keyed by that state.
    both_refused: BTreeMap<String, usize>,
    /// PostgreSQL accepts a generated policy the head refuses `0A000`
    /// (a form `render/definition.rs` cannot yet reproduce), keyed by the
    /// head's own message so each form's count is visible.
    head_refused: BTreeMap<String, usize>,
}

#[test]
fn generated_schemas_dump_from_the_head_exactly_as_from_postgres() {
    let Some(postgres) = Postgres::start() else {
        return;
    };
    let cases = std::env::var("PG_DUMP_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(12);
    let config = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner =
        TestRunner::new_with_rng(config, TestRng::deterministic_rng(RngAlgorithm::ChaCha));
    let outcomes = RefCell::new(Outcomes::default());
    runner
        .run(&schema(), |schema| {
            let generated = statements(&schema);
            check(&postgres, &outcomes, &generated);
            Ok(())
        })
        .expect("every generated schema round trips");
    let outcomes = outcomes.into_inner();
    eprintln!(
        "policy fuzz outcomes: both accept and match = {}; both refuse with the same SQLSTATE = {:?}; the head refuses 0A000 by form = {:?}",
        outcomes.both_accept, outcomes.both_refused, outcomes.head_refused
    );
}

fn check(postgres: &Postgres, outcomes: &RefCell<Outcomes>, generated: &GeneratedStatements) {
    let database = postgres.fresh_database();
    let (head, port) = spawn_head();
    for (role, sql) in &generated.setup {
        let expected = postgres.run_as(&database, role, sql);
        let actual = run_on_head(&head, role, sql);
        assert_eq!(
            actual, expected,
            "the head and PostgreSQL disagree on `{sql}` run as {role}"
        );
    }
    for policy in &generated.policies {
        check_policy(postgres, &database, &head, outcomes, policy);
    }
    let from_postgres = postgres.pg_dump(postgres.port, &database);
    let from_head = postgres.pg_dump(port, "postgres");
    postgres.drop_database(&database);
    assert_eq!(
        from_head, from_postgres,
        "the dumps differ for {generated:?}"
    );
}

/// One `CREATE POLICY`'s own three-way outcome (the task this whole harness
/// exists to check): PostgreSQL and the head must agree whenever either
/// admits the form, and the head's only allowed disagreement is its own
/// typed `0A000` for a form `render/definition.rs` cannot yet reproduce.
/// When that happens, the policy PostgreSQL created is dropped again so
/// the two schemas stay in sync for the byte-for-byte dump comparison
/// `check` still runs unconditionally afterward.
fn check_policy(
    postgres: &Postgres,
    database: &str,
    head: &Head,
    outcomes: &RefCell<Outcomes>,
    policy: &PolicyCase,
) {
    let pg_code = postgres.run_as(database, &policy.owner, &policy.sql);
    let head_result = run_policy_on_head(head, &policy.owner, &policy.sql);
    let head_code = head_result.as_ref().map(|(code, _)| code.clone());
    match (&pg_code, &head_code) {
        (None, None) => outcomes.borrow_mut().both_accept += 1,
        (None, Some(code)) if code.as_str() == "0A000" => {
            let (_, message) = head_result.expect("a 0A000 outcome carries its own message");
            postgres.superuser(
                database,
                &format!(
                    "DROP POLICY {} ON {}",
                    quote(&policy.name),
                    quote(policy.table)
                ),
            );
            *outcomes
                .borrow_mut()
                .head_refused
                .entry(message)
                .or_insert(0) += 1;
        }
        (Some(pg_state), Some(head_state)) if pg_state == head_state => {
            *outcomes
                .borrow_mut()
                .both_refused
                .entry(pg_state.clone())
                .or_insert(0) += 1;
        }
        _ => panic!(
            "policy divergence on `{}` run as {}: PostgreSQL {pg_code:?}, the head {head_code:?}",
            policy.sql, policy.owner
        ),
    }
}

fn schema() -> impl Strategy<Value = Schema> {
    (
        any::<[bool; 3]>(),
        proptest::sample::subsequence(TABLE_NAMES.to_vec(), 1..=3)
            .prop_flat_map(|names| names.into_iter().map(table).collect::<Vec<_>>()),
    )
        .prop_map(|(logins, tables)| Schema { logins, tables })
}

fn table(name: &'static str) -> impl Strategy<Value = Table> {
    (
        proptest::option::of(0..ROLES.len()),
        proptest::sample::subsequence(COLUMN_NAMES.to_vec(), 1..=4).prop_flat_map(|names| {
            names
                .into_iter()
                .map(|name| {
                    (column_type(), any::<bool>()).prop_map(move |(ty, not_null)| Column {
                        name,
                        ty,
                        not_null,
                    })
                })
                .collect::<Vec<_>>()
        }),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_flat_map(move |(owner, columns, enable, force)| {
            let width = columns.len();
            (
                Just(owner),
                Just(columns.clone()),
                proptest::option::of(0..width),
                proptest::collection::vec(grant(), 0..=2),
                Just(enable),
                Just(force),
                proptest::collection::vec(
                    (
                        proptest::option::of(0..ROLES.len()),
                        policy_expression(columns),
                    ),
                    0..=2,
                ),
            )
        })
        .prop_map(
            move |(owner, columns, primary_key, grants, enable, force, policies)| Table {
                name,
                owner,
                columns,
                primary_key,
                grants,
                enable,
                force,
                policies,
            },
        )
}

fn column_type() -> impl Strategy<Value = Type> {
    prop_oneof![Just(Type::Integer), Just(Type::BigInt), Just(Type::Text)]
}

fn grant() -> impl Strategy<Value = (Vec<&'static str>, Option<usize>)> {
    (
        proptest::sample::subsequence(TABLE_PRIVILEGES.to_vec(), 1..=3),
        proptest::option::of(0..ROLES.len()),
    )
        .prop_map(|(privileges, grantee)| {
            if privileges.contains(&"ALL") {
                (vec!["ALL"], grantee)
            } else {
                (privileges, grantee)
            }
        })
}

/// A policy's whole `USING` expression: a boolean tree over comparisons,
/// `IS [NOT] NULL` and `IS [NOT] DISTINCT FROM` on this table's own
/// columns, bounded by `BOOLEAN_DEPTH`. Subqueries are not generated at
/// all: `Position::Policy`'s own rules (`analyze/typing/context.rs`) mark
/// every subquery form `Verdict::NotBuilt` there, so the gate never admits
/// one in a policy regardless of what PostgreSQL 18 itself allows.
fn policy_expression(columns: Vec<Column>) -> impl Strategy<Value = Expr> {
    boolean_expr(columns, BOOLEAN_DEPTH)
}

fn boolean_expr(columns: Vec<Column>, depth: u32) -> BoxedStrategy<Expr> {
    let leaf = leaf_predicate(columns.clone(), OPERAND_DEPTH);
    if depth == 0 {
        return leaf;
    }
    let inner = boolean_expr(columns, depth - 1);
    prop_oneof![
        4 => leaf,
        2 => proptest::collection::vec(inner.clone(), 2..=3).prop_map(Expr::And),
        2 => proptest::collection::vec(inner.clone(), 2..=3).prop_map(Expr::Or),
        1 => inner.prop_map(|expr| Expr::Not(Box::new(expr))),
    ]
    .boxed()
}

/// One comparison, `IS [NOT] NULL` or `IS [NOT] DISTINCT FROM` anchored on
/// a column of this table, the other side an `operand` of the same type
/// built with `operand_depth` budget: `operand`'s own `CASE` branch calls
/// this with its remaining budget (never the top-level `OPERAND_DEPTH`
/// again), so a `CASE` condition's own operands shrink the same way every
/// other nested operand does, and construction always terminates.
fn leaf_predicate(columns: Vec<Column>, operand_depth: u32) -> BoxedStrategy<Expr> {
    let all_columns = columns.clone();
    proptest::sample::select(columns)
        .prop_flat_map(move |column| {
            let ty = column.ty;
            let name = column.name;
            let all_columns = all_columns.clone();
            prop_oneof![
                3 => (compare_op(), operand(ty, all_columns.clone(), operand_depth))
                    .prop_map(move |(op, other)| Expr::Compare(op, Operand::Column(name), other)),
                1 => any::<bool>()
                    .prop_map(move |negated| Expr::IsNull(Operand::Column(name), negated)),
                1 => (operand(ty, all_columns, operand_depth), any::<bool>()).prop_map(
                    move |(other, negated)| Expr::IsDistinct(Operand::Column(name), other, negated)
                ),
            ]
        })
        .boxed()
}

/// An operand of exactly `ty`: a column of that type, a literal, `NULL`,
/// (for `text`) `current_user`/`current_setting(...)`, or, while `depth`
/// budget remains, a `CAST` of some other typed operand into `ty` or a
/// `CASE` whose arms are all `ty`-typed. Every generated cast is one
/// PostgreSQL 18 itself permits between the three declarable column types,
/// so a refusal here is always a fact about the head, never the grammar.
fn operand(ty: Type, columns: Vec<Column>, depth: u32) -> BoxedStrategy<Operand> {
    let leaf = leaf_operand(ty, columns.clone());
    if depth == 0 {
        return leaf;
    }
    let inner = operand(ty, columns.clone(), depth - 1);
    let cast_ty = cast_type_of(ty);
    let cast = operand_of_any_type(columns.clone(), depth - 1)
        .prop_map(move |value| Operand::Cast(Box::new(value), cast_ty));
    let case = (
        proptest::collection::vec((leaf_predicate(columns, depth - 1), inner.clone()), 1..=2),
        inner,
    )
        .prop_map(|(arms, otherwise)| Operand::Case(arms, Box::new(otherwise)));
    prop_oneof![
        5 => leaf,
        1 => cast,
        1 => case,
    ]
    .boxed()
}

/// A `CAST` source: an operand of some declarable type, not necessarily
/// `ty`, since PostgreSQL 18 allows casting any of the three declarable
/// column types to any other.
fn operand_of_any_type(columns: Vec<Column>, depth: u32) -> BoxedStrategy<Operand> {
    column_type()
        .prop_flat_map(move |ty| operand(ty, columns.clone(), depth))
        .boxed()
}

fn leaf_operand(ty: Type, columns: Vec<Column>) -> BoxedStrategy<Operand> {
    let matching: Vec<&'static str> = columns
        .iter()
        .filter(|column| column.ty == ty)
        .map(|column| column.name)
        .collect();
    let mut branches: Vec<(u32, BoxedStrategy<Operand>)> = Vec::new();
    if !matching.is_empty() {
        branches.push((
            3,
            proptest::sample::select(matching)
                .prop_map(Operand::Column)
                .boxed(),
        ));
    }
    branches.push((1, Just(Operand::Null).boxed()));
    match ty {
        Type::Integer => {
            branches.push((3, (-5i64..2_000_000_000).prop_map(Operand::Integer).boxed()));
            branches.push((1, (-5i64..100).prop_map(Operand::QuotedInteger).boxed()));
        }
        Type::BigInt => {
            branches.push((
                3,
                (-3_000_000_000i64..3_000_000_000)
                    .prop_map(Operand::Integer)
                    .boxed(),
            ));
            branches.push((1, (-5i64..100).prop_map(Operand::QuotedInteger).boxed()));
        }
        Type::Text => {
            branches.push((3, "[a-z' ]{0,6}".prop_map(Operand::Text).boxed()));
            branches.push((1, Just(Operand::CurrentUser).boxed()));
            branches.push((1, "[a-z_.]{1,8}".prop_map(Operand::CurrentSetting).boxed()));
        }
    }
    Union::new_weighted(branches).boxed()
}

#[derive(Debug)]
struct PolicyCase {
    owner: String,
    table: &'static str,
    name: String,
    sql: String,
}

#[derive(Debug)]
struct GeneratedStatements {
    setup: Vec<(String, String)>,
    policies: Vec<PolicyCase>,
}

fn statements(schema: &Schema) -> GeneratedStatements {
    let postgres = |sql: String| ("postgres".to_string(), sql);
    let mut setup = ROLES
        .iter()
        .zip(schema.logins)
        .map(|(role, login)| {
            postgres(format!(
                "CREATE ROLE {}{}",
                quote(role),
                if login { " LOGIN" } else { "" }
            ))
        })
        .collect::<Vec<_>>();
    let mut policies = Vec::new();
    for table in &schema.tables {
        let owner = match table.owner {
            Some(role) if schema.logins[role] => {
                setup.push(postgres(format!(
                    "GRANT CREATE ON SCHEMA public TO {}",
                    quote(ROLES[role])
                )));
                ROLES[role].to_string()
            }
            _ => "postgres".to_string(),
        };
        let columns = table
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let mut definition = format!("{} {}", quote(column.name), type_name(column.ty));
                if table.primary_key == Some(index) {
                    definition.push_str(" PRIMARY KEY");
                } else if column.not_null {
                    definition.push_str(" NOT NULL");
                }
                definition
            })
            .collect::<Vec<_>>()
            .join(", ");
        let as_owner = |sql: String| (owner.clone(), sql);
        setup.push(as_owner(format!(
            "CREATE TABLE {} ({columns})",
            quote(table.name)
        )));
        for (privileges, grantee) in &table.grants {
            setup.push(as_owner(format!(
                "GRANT {} ON {} TO {}",
                privileges.join(", "),
                quote(table.name),
                grantee.map_or("PUBLIC".to_string(), |role| quote(ROLES[role]))
            )));
        }
        if table.enable {
            setup.push(as_owner(format!(
                "ALTER TABLE {} ENABLE ROW LEVEL SECURITY",
                quote(table.name)
            )));
        }
        if table.force {
            setup.push(as_owner(format!(
                "ALTER TABLE {} FORCE ROW LEVEL SECURITY",
                quote(table.name)
            )));
        }
        for (index, (role, expression)) in table.policies.iter().enumerate() {
            let target = role.map_or(String::new(), |role| format!(" TO {}", quote(ROLES[role])));
            let name = format!("p{index}");
            let sql = format!(
                "CREATE POLICY {name} ON {} FOR SELECT{target} USING ({})",
                quote(table.name),
                render(expression)
            );
            policies.push(PolicyCase {
                owner: owner.clone(),
                table: table.name,
                name,
                sql,
            });
        }
    }
    GeneratedStatements { setup, policies }
}

fn render(expr: &Expr) -> String {
    match expr {
        Expr::Compare(op, left, right) => {
            format!("{} {} {}", operand_sql(left), op.sql(), operand_sql(right))
        }
        Expr::IsNull(value, negated) => format!(
            "{} IS {}NULL",
            operand_sql(value),
            if *negated { "NOT " } else { "" }
        ),
        Expr::IsDistinct(left, right, negated) => format!(
            "{} IS {}DISTINCT FROM {}",
            operand_sql(left),
            if *negated { "NOT " } else { "" },
            operand_sql(right)
        ),
        Expr::Not(inner) => format!("NOT ({})", render(inner)),
        Expr::And(parts) => group(parts, " AND "),
        Expr::Or(parts) => group(parts, " OR "),
    }
}

fn group(parts: &[Expr], separator: &str) -> String {
    let parts = parts
        .iter()
        .map(|part| format!("({})", render(part)))
        .collect::<Vec<_>>();
    parts.join(separator)
}

fn operand_sql(operand: &Operand) -> String {
    match operand {
        Operand::Column(name) => quote(name),
        Operand::Integer(value) => value.to_string(),
        Operand::QuotedInteger(value) => format!("'{value}'"),
        Operand::Text(text) => format!("'{}'", text.replace('\'', "''")),
        Operand::Null => "NULL".to_string(),
        Operand::CurrentUser => "current_user".to_string(),
        Operand::CurrentSetting(name) => format!("current_setting('{}')", name.replace('\'', "''")),
        Operand::Cast(inner, ty) => format!("({})::{}", operand_sql(inner), ty.sql()),
        Operand::Case(arms, otherwise) => {
            let arms = arms
                .iter()
                .map(|(condition, then)| {
                    format!("WHEN {} THEN {}", render(condition), operand_sql(then))
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("CASE {arms} ELSE {} END", operand_sql(otherwise))
        }
    }
}

fn type_name(ty: Type) -> &'static str {
    match ty {
        Type::Integer => "integer",
        Type::BigInt => "bigint",
        Type::Text => "text",
    }
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn run_on_head(head: &Head, role: &str, sql: &str) -> Option<String> {
    let session = head
        .connect(role)
        .expect("the acting role can log in to the head");
    session
        .execute(sql)
        .err()
        .map(|error| error.state.code().to_string())
}

/// Like `run_on_head`, but keeps the error's own message too, so a `0A000`
/// refusal can be bucketed by which form it names.
fn run_policy_on_head(head: &Head, role: &str, sql: &str) -> Option<(String, String)> {
    let session = head
        .connect(role)
        .expect("the acting role can log in to the head");
    session
        .execute(sql)
        .err()
        .map(|error| (error.state.code().to_string(), error.message))
}

fn error_code(events: &[BackendEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        BackendEvent::ErrorResponse(fields) => fields.get(&b'C').cloned(),
        BackendEvent::RowDescription(_)
        | BackendEvent::DataRow(_)
        | BackendEvent::CommandComplete(_)
        | BackendEvent::ReadyForQuery(_)
        | BackendEvent::Other(_) => None,
    })
}

fn spawn_head() -> (Arc<Head>, u16) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = format!("generated-dump-{}.db", NEXT.fetch_add(1, Ordering::Relaxed));
    let head = Arc::new(Head::open(Arc::new(MemoryIO::new()), &path).expect("the head opens"));
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("a bound address").port();
    listener
        .set_nonblocking(true)
        .expect("the listener can be non-blocking");
    let server_head = head.clone();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("a runtime starts")
            .block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("tokio adopts the listener");
                serve(server_head, listener).await
            })
    });
    (head, port)
}

struct Postgres {
    bin: PathBuf,
    data: tempfile::TempDir,
    port: u16,
}

impl Postgres {
    fn start() -> Option<Postgres> {
        let bin = common::require_postgres(
            "the generated pg_dump round trip",
            &["initdb", "pg_ctl", "pg_dump"],
        )?;
        let data = tempfile::tempdir().expect("a scratch directory");
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("a free port")
            .local_addr()
            .expect("a bound address")
            .port();
        run(Command::new(bin.join("initdb"))
            .args(["-U", "postgres", "--auth=trust", "-D"])
            .arg(data.path().join("cluster")));
        run(Command::new(bin.join("pg_ctl"))
            .arg("-D")
            .arg(data.path().join("cluster"))
            .arg("-l")
            .arg(data.path().join("log"))
            .args([
                "-o",
                &format!("-p {port} -h 127.0.0.1 -k /tmp"),
                "-w",
                "start",
            ]));
        Some(Postgres { bin, data, port })
    }

    fn fresh_database(&self) -> String {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let database = format!("case_{}", NEXT.fetch_add(1, Ordering::Relaxed));
        self.superuser("postgres", &format!("CREATE DATABASE {database}"));
        database
    }

    fn drop_database(&self, database: &str) {
        self.superuser("postgres", &format!("DROP DATABASE {database}"));
        for role in ROLES {
            self.superuser("postgres", &format!("DROP ROLE IF EXISTS {}", quote(role)));
        }
    }

    fn run_as(&self, database: &str, role: &str, sql: &str) -> Option<String> {
        let mut conn = self.connect(database);
        if role != "postgres" {
            let set = conn
                .simple_query(&format!("SET ROLE {}", quote(role)))
                .expect("SET ROLE runs");
            assert_eq!(error_code(&set), None, "SET ROLE {role} failed");
        }
        error_code(&conn.simple_query(sql).expect(sql))
    }

    fn superuser(&self, database: &str, sql: &str) {
        let events = self.connect(database).simple_query(sql).expect(sql);
        assert_eq!(error_code(&events), None, "{sql} failed on PostgreSQL");
    }

    fn connect(&self, database: &str) -> PgConn {
        let params = ConnParams::parse(&format!(
            "postgres://postgres@127.0.0.1:{}/{database}",
            self.port
        ));
        PgConn::connect(&params).expect("postgres can log in")
    }

    fn pg_dump(&self, port: u16, database: &str) -> String {
        let output = Command::new(self.bin.join("pg_dump"))
            .args(["--schema-only", "-h", "127.0.0.1", "-U", "postgres"])
            .args(["-p", &port.to_string(), database])
            .arg(format!("--restrict-key={RESTRICT_KEY}"))
            .output()
            .expect("pg_dump runs");
        assert!(
            output.status.success(),
            "pg_dump failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("pg_dump writes UTF-8")
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl"))
            .arg("-D")
            .arg(self.data.path().join("cluster"))
            .args(["-m", "immediate", "stop"])
            .output();
    }
}

/// Fixed on both sides (the head's own `pg_dump` and PostgreSQL's) so the
/// `\restrict`/`\unrestrict` lines pg_dump 17.6+/18 emit with an otherwise
/// random key compare byte-for-byte instead of needing to be filtered out.
const RESTRICT_KEY: &str = "tursopgheadpgdumpgeneratedtestrestrictkey0";

fn run(command: &mut Command) {
    let output = command.output().expect("the PostgreSQL tool runs");
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
