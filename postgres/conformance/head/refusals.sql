--
-- Head refusals
--
-- Statements PostgreSQL 18 admits (checked against a live server) but the
-- head refuses by design, because the feature is outside its closed,
-- admitted set. Head-authored: recorded from a fresh pg-head-server, a
-- reviewed snapshot, never from PostgreSQL. Do not mix this file's
-- expectations with a PostgreSQL-recorded one.
--

-- refused by name: outside the admitted statement set
CREATE TABLE refusal_notes (id integer);
DROP TABLE refusal_notes;

-- a column default is not supported
CREATE TABLE t_default (id integer DEFAULT 1);

-- a column UNIQUE constraint is not supported
CREATE TABLE t_unique (id integer UNIQUE);

-- a table constraint is not supported (only a single-column PRIMARY KEY on
-- the column definition itself is)
CREATE TABLE t_tableconstraint (id integer, PRIMARY KEY (id));

-- a column's STORAGE clause is not supported
CREATE TABLE t_storage (id integer PRIMARY KEY, body text STORAGE EXTERNAL);

-- a named PRIMARY KEY constraint is not supported (its catalog name is
-- always synthesized, matching PostgreSQL's own default naming)
CREATE TABLE t_namedpk (id integer CONSTRAINT my_pk PRIMARY KEY);

-- a PRIMARY KEY's own storage parameter (WITH (...)) is not supported
CREATE TABLE t_pkopts (id integer PRIMARY KEY WITH (fillfactor=70));

-- a PRIMARY KEY's own index tablespace (USING INDEX TABLESPACE) is not
-- supported
CREATE TABLE t_pktbs (id integer PRIMARY KEY USING INDEX TABLESPACE pg_default);

-- IF NOT EXISTS is not supported
CREATE TABLE IF NOT EXISTS t_ifnotexists (id integer);

-- TEMPORARY tables are not supported
CREATE TEMP TABLE t_temp (id integer);

-- a column type other than integer, bigint or text is not supported
CREATE TABLE t_nametype (id name);

-- a type modifier is not supported
CREATE TABLE t_typemod (name varchar(10));

-- a schema other than public or pg_catalog is not supported
CREATE TABLE other.t_otherschema (id integer);

-- an identifier longer than 63 bytes is not supported (PostgreSQL instead
-- truncates it and succeeds; see README.md)
CREATE TABLE xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx (id integer);

-- a Unicode escape identifier is not supported
CREATE TABLE U&"notes2" (id integer);

--
-- SELECT: head refusals
--
-- Converted from pg/head/tests/queries.rs.
--

CREATE TABLE rf_dept (id integer PRIMARY KEY, name text);
CREATE TABLE rf_emp (id integer PRIMARY KEY, dept_id integer, name text);
INSERT INTO rf_dept VALUES (1, 'eng');
INSERT INTO rf_emp VALUES (1, 1, 'alice');

-- DISTINCT ON is not supported
SELECT DISTINCT ON (dept_id) name FROM rf_emp;

-- LATERAL is not supported
SELECT * FROM rf_emp JOIN LATERAL (SELECT 1) AS x ON true;

-- NATURAL JOIN is not supported
SELECT * FROM rf_emp NATURAL JOIN rf_dept;

-- JOIN ... USING is not supported
SELECT * FROM rf_emp JOIN rf_dept USING (id);

-- an alias on a joined table is not supported (the head has no way to hide
-- the inner tables' own names the way PostgreSQL does once one is given)
SELECT * FROM (rf_emp JOIN rf_dept ON rf_emp.dept_id = rf_dept.id) AS x;

-- a FROM-clause subquery without an alias is refused (PostgreSQL 18 itself
-- admits this; the head keeps the older SQL-92 requirement, so this is the
-- one entry here refused as a syntax error rather than 0A000)
SELECT * FROM (SELECT id FROM rf_dept);

-- ROWS FROM with more than one function is not supported
SELECT * FROM ROWS FROM (unnest('{1}'::oid[]), unnest('{2}'::oid[])) AS x(a, b);

-- unnest over more than one array argument is not supported (PostgreSQL 18
-- has its own multi-array unnest(anyarray, anyarray, ...) overload; the
-- head has no such overload and refuses by name instead)
SELECT * FROM unnest('{1,2}'::pg_catalog.oid[], '{3,4}'::pg_catalog.oid[]) AS x(v);

--
-- expressions: head refusals
--
-- Converted from pg/head/tests/expressions.rs.
--

CREATE ROLE rf_bob2 LOGIN;
CREATE TABLE rf_expr_t (id integer PRIMARY KEY);
GRANT SELECT ON rf_expr_t TO rf_bob2;

-- an IN list in a policy's USING expression is not supported (it stores as
-- PostgreSQL's own "= ANY (ARRAY[...])" text, which this parser does not
-- admit back in)
CREATE POLICY p ON rf_expr_t FOR SELECT TO rf_bob2 USING (id IN (4, 5, 6));
CREATE POLICY q ON rf_expr_t FOR SELECT TO rf_bob2 USING (id NOT IN (1, 2, 3));

-- INSERT VALUES admits only a literal or a cast of a literal, not an
-- arbitrary expression
CREATE TABLE rf_expr_scratch (id integer PRIMARY KEY, flag text);
INSERT INTO rf_expr_scratch VALUES (5, CASE WHEN 5 = 5 THEN 't' ELSE 'f' END);

--
-- pg_get_functions: head refusals
--
-- Converted from pg/head/tests/pg_get_functions.rs.
--

CREATE TABLE rf_fg_t (id integer PRIMARY KEY);

-- a catalog-rendered function is refused outside a select list (kept short:
-- see this file's expression-positions section on the LINE clip)
SELECT 1 WHERE pg_get_constraintdef(1::oid)='x';

-- tableoid in a policy's USING expression is not supported: this renderer
-- cannot reproduce PostgreSQL 18's pg_get_expr text for it, so it is
-- refused at CREATE POLICY time instead of stored wrong
CREATE POLICY r ON rf_fg_t FOR SELECT USING (tableoid = 0);

--
-- pg_catalog: head refusals
--
-- Converted from pg/head/tests/pg_catalog.rs.
--

CREATE ROLE rf_pc_alice LOGIN;

-- GRANT on a catalog relation is not supported
GRANT SELECT ON pg_class TO rf_pc_alice;

-- a regproc cast from a name is not supported
SELECT 'pg_is_in_recovery'::regproc;

--
-- session: head refusals
--
-- Converted from pg/head/tests/session.rs.
--

CREATE TABLE rf_sn_t (id integer PRIMARY KEY, owner text);
INSERT INTO rf_sn_t VALUES (1, 'x');

-- SET statement_timeout to a nonzero value is not supported
SET statement_timeout = 5000;

-- only the serializable, read committed and read uncommitted isolation
-- levels are refused; repeatable read (the engine's own isolation level)
-- is admitted elsewhere
BEGIN;
SET TRANSACTION ISOLATION LEVEL SERIALIZABLE;
ROLLBACK;
BEGIN;
SET TRANSACTION ISOLATION LEVEL READ COMMITTED;
ROLLBACK;
BEGIN;
SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED;
ROLLBACK;
BEGIN ISOLATION LEVEL SERIALIZABLE;
ROLLBACK;

-- LOCK TABLE in a mode other than ACCESS SHARE is not supported
BEGIN;
LOCK TABLE rf_sn_t IN EXCLUSIVE MODE;
ROLLBACK;

-- a non-constant argument to a session function outside a top-level
-- select item is not supported
SELECT 1 FROM rf_sn_t WHERE current_setting(owner) = 'x';

-- a dotted, unrecognized name is refused like any other unrecognized
-- setting (PostgreSQL 18 silently accepts one as a placeholder GUC, used
-- by extensions and PostgREST-style JWT claim settings; storing an
-- arbitrary placeholder GUC's value is a real feature the head does not
-- have yet)
SET request.jwt.claims = '{}';

--
-- rows: head refusals
--
-- Converted from pg/head/tests/rows.rs.
--

-- SELECT with no columns is not supported
SELECT FROM rf_emp;

-- the ~ operator is not supported
SELECT * FROM rf_emp WHERE name ~ 'x';

-- SELECT with FOR UPDATE or FOR SHARE is not supported
SELECT * FROM rf_emp FOR UPDATE;

--
-- GRANT and role forms: head refusals
--
-- Converted from pg/head/tests/grants.rs.
--

CREATE ROLE rf_gr_bob LOGIN;
CREATE TABLE rf_gr_notes (id integer PRIMARY KEY, body text);

-- a column privilege is not supported
GRANT SELECT (id) ON rf_gr_notes TO rf_gr_bob;

-- GRANT with GRANT OPTION is not supported
GRANT SELECT ON rf_gr_notes TO rf_gr_bob WITH GRANT OPTION;

-- GRANT to CURRENT_USER (or CURRENT_ROLE, SESSION_USER) is not supported
GRANT SELECT ON rf_gr_notes TO CURRENT_USER;

-- REVOKE is not supported
REVOKE SELECT ON rf_gr_notes FROM rf_gr_bob;

-- the role option SUPERUSER is not supported
CREATE ROLE rf_gr_carol SUPERUSER;

-- CREATE USER and CREATE GROUP are not supported
CREATE USER rf_gr_dave;

--
-- Row-level security forms: head refusals
--
-- Converted from pg/head/tests/row_security.rs.
--

CREATE ROLE rf_rs_bob LOGIN;
CREATE TABLE rf_rs_notes (id integer PRIMARY KEY);

-- CREATE POLICY for commands other than SELECT, including an omitted FOR,
-- is not supported
CREATE POLICY rf_rs_p1 ON rf_rs_notes USING (id = 1);
CREATE POLICY rf_rs_p2 ON rf_rs_notes FOR INSERT WITH CHECK (id = 1);

-- AS RESTRICTIVE is not supported
CREATE POLICY rf_rs_p3 ON rf_rs_notes AS RESTRICTIVE FOR SELECT USING (id = 1);

-- CREATE POLICY without USING is not supported
CREATE POLICY rf_rs_p4 ON rf_rs_notes FOR SELECT;

-- PUBLIC combined with other roles is not supported
CREATE POLICY rf_rs_p5 ON rf_rs_notes FOR SELECT TO PUBLIC, rf_rs_bob USING (id = 1);

-- ALTER TABLE subcommands other than ROW LEVEL SECURITY are not supported
ALTER TABLE rf_rs_notes ADD COLUMN extra text;

-- ALTER TABLE IF EXISTS is not supported
ALTER TABLE IF EXISTS rf_rs_notes ENABLE ROW LEVEL SECURITY;

--
-- Expression positions: head refusals
--
-- Converted from pg/head/tests/expression_positions.rs: PostgreSQL 18
-- admits each statement below (probed live); the head does not implement
-- the position/feature combination.
--

-- Table and column names are kept short: a position-bearing statement
-- must stay at or under 60 characters or real psql clips its LINE excerpt
-- with "...", which the runner does not yet emulate (see README.md).
CREATE TABLE r (a integer PRIMARY KEY, b integer);
CREATE TABLE e2 (x integer PRIMARY KEY);
INSERT INTO r VALUES (1, 10), (2, 20);

-- a catalog-rendered function outside a top-level SELECT target: WHERE,
-- JOIN/ON, ORDER BY, EXECUTE parameter, a policy's USING expression, and a
-- function's own FROM argument (this last one needs the runner's LINE
-- clipping: PostgreSQL 18 itself admits it, so the shortest statement that
-- still calls the function types its argument as an undefined function
-- instead unless it is long enough to reach the position with a real
-- integer-returning constraint description in between).
SELECT * FROM r WHERE pg_get_constraintdef(1::oid)='x';
SELECT * FROM r JOIN e2 ON pg_get_constraintdef(1::oid)='x';
SELECT a FROM r ORDER BY pg_get_constraintdef(1::oid);
PREPARE re3(text) AS SELECT $1;
EXECUTE re3(pg_get_constraintdef(1::oid));
CREATE POLICY rep ON r FOR SELECT USING (pg_get_constraintdef(1::oid) = 'x');
SELECT * FROM generate_series(1, pg_get_constraintdef(1::oid)::integer);

-- a set-returning function outside FROM: a fresh SELECT target and ORDER BY
-- expression each multiply rows in PostgreSQL; the head does not implement
-- either
SELECT generate_series(1, 2) FROM r;
SELECT a FROM r ORDER BY generate_series(1, 2);

-- an aggregate as a fresh ORDER BY expression (as opposed to a reference to
-- an already-typed SELECT target) is not implemented
SELECT 1 FROM r ORDER BY array_agg(a);

-- a subquery outside WHERE/JOIN/ON/SELECT target/ORDER BY: INSERT VALUES
-- and a policy's USING expression
INSERT INTO r (a) VALUES ((SELECT 1));
CREATE POLICY resub ON r FOR SELECT USING ((SELECT true));

-- a cast of a catalog-rendered function's result in INSERT VALUES
CREATE TABLE rz (id integer PRIMARY KEY, c text);
INSERT INTO rz(id,c) VALUES(1,pg_get_constraintdef(1::oid));

-- a set-returning function in INSERT VALUES
CREATE TABLE rz2 (a integer);
INSERT INTO rz2 (a) VALUES (generate_series(10, 11));

--
-- refusals.rs
--
-- Converted from pg/head/tests/refusals.rs.
--

CREATE TABLE rf_jn_dept (id integer PRIMARY KEY, name text);
CREATE TABLE rf_jn_emp (id integer PRIMARY KEY, dept_id integer, name text);
INSERT INTO rf_jn_dept VALUES (1, 'eng'), (2, 'sales');
INSERT INTO rf_jn_emp VALUES (1, 1, 'alice'), (2, 1, 'bob'), (3, NULL, 'carol');

-- RIGHT JOIN and FULL JOIN are not supported
SELECT e.name, d.name FROM rf_jn_emp e RIGHT JOIN rf_jn_dept d ON e.dept_id = d.id;
SELECT e.name, d.name FROM rf_jn_emp e FULL JOIN rf_jn_dept d ON e.dept_id = d.id;

-- a deduplicating UNION is not supported (UNION ALL is; see queries.sql)
SELECT name FROM rf_jn_dept UNION SELECT name FROM rf_jn_dept;

-- LIMIT and OFFSET are not supported
SELECT name FROM rf_jn_emp ORDER BY name LIMIT 1;
SELECT name FROM rf_jn_emp ORDER BY name OFFSET 1;

-- an aggregate's own * is not supported; count itself is not a registered
-- aggregate, so a real argument is an undefined-function error instead
SELECT count(*) FROM rf_jn_emp;
SELECT count(dept_id) FROM rf_jn_emp;

-- SHOW and RESET are not supported
SHOW search_path;
RESET search_path;
RESET ALL;

-- DEALLOCATE (by name or ALL) is not supported
PREPARE rf_jn_p AS SELECT 1;
DEALLOCATE rf_jn_p;
DEALLOCATE ALL;

--
-- authmatrix.sql gap reasons
--
-- One probe per reason the other 270 of authmatrix's 370 PostgreSQL-
-- recorded scenarios cannot run on the head, with how many scenarios
-- each reason blocks. Each probe must still be refused; when one starts
-- succeeding, restore that reason's scenarios into authmatrix.sql from
-- pg/head/tests/authmatrix/cells.rs in the parent of the commit "pg:
-- move grants, row security, authmatrix and prepared statements to the
-- corpus", which deleted it.
--

CREATE ROLE rf_am_owner LOGIN;
CREATE ROLE rf_am_grantee LOGIN;

-- the CREATEROLE role attribute is not supported (2 scenarios)
CREATE ROLE rf_am_createrole CREATEROLE;

-- role membership (GRANT of a role to a role) is not supported (68
-- scenarios, plus 12 that also need BYPASSRLS)
GRANT rf_am_owner TO rf_am_grantee;

-- the BYPASSRLS role attribute is not supported (24 scenarios)
CREATE ROLE rf_am_bypassrls BYPASSRLS;

-- reassigning a schema's owner is not supported (56 scenarios)
ALTER SCHEMA public OWNER TO rf_am_owner;

-- CREATE VIEW is not supported (30 scenarios)
CREATE VIEW rf_am_view AS SELECT 1;

-- CREATE INDEX is not supported (30 scenarios)
CREATE INDEX rf_am_index ON rf_am_index_t (id);

-- CREATE SEQUENCE is not supported (30 scenarios)
CREATE SEQUENCE rf_am_sequence;

-- a schema other than public or pg_catalog is not supported (18
-- scenarios)
GRANT SELECT ON rf_am_s.rf_am_absent TO rf_am_grantee;
