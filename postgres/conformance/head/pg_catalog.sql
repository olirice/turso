--
-- pg_catalog: relation lookup, ACLs, regclass/regproc, name/oidvector/
-- int2vector subscripting
--
-- Converted from pg/head/tests/pg_catalog.rs (with three refused-by-name
-- cases in refusals.sql). Recorded from real PostgreSQL 18 (see
-- README.md); never edited by hand. Names carry a pc_ prefix so they
-- cannot collide with another file's fixtures in the shared schedule
-- database.
--

-- pg_class resolves both unqualified and pg_catalog-qualified
SELECT relname FROM pg_class WHERE oid = 1259;
SELECT relname FROM pg_catalog.pg_class WHERE oid = 1259;

-- a project table shows up in pg_class alongside the constant rows: the
-- constant layer (the built-in tables) and the project layer (one user
-- table) both contribute rows to the same relation
CREATE TABLE pc_notes (id integer);
SELECT relname FROM pg_class WHERE relname = 'pc_notes';
SELECT true FROM pg_class WHERE relname = 'pg_class';
SELECT true FROM pg_class WHERE relname = 'pc_notes';

-- a non-superuser is refused pg_authid but reads pg_class
CREATE ROLE pc_alice LOGIN;
\c - pc_alice
SELECT relname FROM pg_class WHERE oid = 1259;
SELECT * FROM pg_authid;
\c - postgres

-- relacl renders PostgreSQL's own text after a GRANT
CREATE TABLE pc_notes2 (id integer);
GRANT SELECT ON pc_notes2 TO pc_alice;
SELECT relacl FROM pg_class WHERE relname = 'pc_notes2';

-- an unqualified name still prefers the pg_catalog relation over a
-- same-named public table: only a superuser has CREATE on the public
-- schema by default, and creating a table literally named pg_class in
-- public is otherwise ordinary, a distinct relation from
-- pg_catalog.pg_class
CREATE TABLE pg_class (x integer);
SELECT relname FROM pg_class WHERE relname = 'pg_class';
SELECT x FROM public.pg_class;

-- creating a table in pg_catalog is refused the way PostgreSQL 18 itself
-- refuses it: system catalog modifications are disallowed
CREATE TABLE pg_catalog.pc_t (id integer);

-- tableoid reports the relation's own oid, and is absent from a zero-row
-- result the same as any other column
SELECT tableoid, oid FROM pg_class WHERE oid = 1259;
SELECT x.tableoid FROM pg_seclabels x;

-- a char literal cast inside a CASE renders as text, not a raw encoded byte
SELECT CASE WHEN 1 = 0 THEN 'x'::"char" ELSE ' '::"char" END;

-- a name column subscripts zero-based like a fixed char array
SELECT typname[0], typname[1] FROM pg_type WHERE typname = 'bool';

CREATE TABLE "héllo" (id integer);

-- a name column subscript is byte-exact, not character-exact
SELECT relname[0], relname[1], relname[2] FROM pg_class WHERE relname = 'héllo';

CREATE TABLE pc_bytes_probe (id integer);

-- a name column subscript past the stored bytes is empty, not the next
-- character, and past NAMEDATALEN is NULL
SELECT relname[10] FROM pg_class WHERE relname = 'pc_bytes_probe';
SELECT relname[100] FROM pg_class WHERE relname = 'pc_bytes_probe';

-- regclass resolves a system relation's name to its oid and back, and a
-- schema-qualified one
SELECT 'pg_class'::regclass::oid;
SELECT (1259::oid)::regclass;
SELECT 'pg_catalog.pg_class'::pg_catalog.regclass::oid;

CREATE TABLE "Mixed Case" (id integer PRIMARY KEY);

-- regclass quotes a mixed-case relation name on the way out
SELECT '"Mixed Case"'::regclass;

-- a regclass cast of a missing relation, and of a missing
-- schema-qualified relation (which names the schema in the error)
SELECT 'no_such_relation_xyz'::regclass;
SELECT 'pg_catalog.nope'::regclass;

CREATE TABLE pc_t (id integer PRIMARY KEY);

-- regclass resolves a quoted, dotted and fully quoted relation name the
-- same way, folds an unquoted name to lower case, and trims surrounding
-- and internal whitespace around a dot: every spelling below resolves to
-- the same oid as the plain unqualified name (a freshly allocated oid, so
-- compared to itself rather than shown, which would never byte-match
-- across two independent databases)
SELECT ('"public".pc_t'::regclass::oid) = ('pc_t'::regclass::oid);
SELECT ('"public"."pc_t"'::regclass::oid) = ('pc_t'::regclass::oid);
SELECT ('PC_T'::regclass::oid) = ('pc_t'::regclass::oid);
SELECT (' public . pc_t '::regclass::oid) = ('pc_t'::regclass::oid);

CREATE TABLE "café" (id integer PRIMARY KEY);

-- regclass accepts a non-ASCII unquoted identifier, resolving to the same
-- oid as the quoted spelling
SELECT ('café'::regclass::oid) = ('"café"'::regclass::oid);

-- regproc renders the captured function name for an oid
SELECT 3810::regproc;

-- a char cast of a constant decodes to its single-character text
SELECT 'abc'::"char";
SELECT ''::"char";

CREATE TABLE pc_notes3 (id integer PRIMARY KEY, body text);
INSERT INTO pc_notes3 VALUES (1, 'abc');

-- a char cast of a column decodes to its single-character text
SELECT body::"char" FROM pc_notes3 WHERE id = 1;

-- two independently folded char literals compare equal or not the same
-- as PostgreSQL's own byte compare
SELECT 'a'::"char" = 'a'::"char";
SELECT 'a'::"char" = 'b'::"char";

-- reltuples is never analyzed, so it always reports -1 for a table and 0
-- for its own index, rendered through this head's general float4 text
-- output
CREATE TABLE pc_reltuples (id integer PRIMARY KEY);
SELECT reltuples FROM pg_class WHERE relname = 'pc_reltuples';
SELECT reltuples FROM pg_class WHERE relname = 'pc_reltuples_pkey';

-- ANY over an array with NULLs follows three-valued logic
SELECT 2 = ANY('{1,2,NULL}'::pg_catalog.oid[]);
SELECT 5 = ANY('{1,2,NULL}'::pg_catalog.oid[]);
SELECT 5 = ANY('{1,2,3}'::pg_catalog.oid[]);
SELECT NULL::oid = ANY('{}'::pg_catalog.oid[]);

-- oidvector subscripts are zero-based
SELECT proargtypes[0], proargtypes[1], proargtypes[2]
FROM pg_proc WHERE proname = 'set_config';

-- ARRAY of a zero-row select is the empty array, not NULL
SELECT ARRAY(SELECT oid FROM pg_type WHERE typname = 'no_such_type') IS NULL;
SELECT v FROM unnest(ARRAY(SELECT oid FROM pg_type WHERE typname = 'int4')) AS x(v);

-- ARRAY of an expression select names its column for the aggregate wrapper
SELECT ARRAY(SELECT typname || '!' FROM pg_type WHERE typname = 'int4');

CREATE TABLE pc_t2 (id integer PRIMARY KEY);

-- int2vector subscripts are zero-based through a union of the constant and
-- project layers, and renders as PostgreSQL's own space-separated text
SELECT indkey[0] FROM pg_index WHERE indrelid = 'pc_t2'::regclass;
SELECT indkey FROM pg_index WHERE indrelid = 'pc_t2'::regclass;

-- oidvector renders as PostgreSQL's own space-separated text, empty when
-- there are no arguments
SELECT proargtypes FROM pg_proc WHERE proname = 'set_config';
SELECT proargtypes FROM pg_proc WHERE proname = 'pg_is_in_recovery';

-- array_upper of an empty array is NULL
SELECT array_upper('{}'::pg_catalog.int2[], 1);
SELECT array_upper('{1,2,3}'::pg_catalog.int2[], 1);
