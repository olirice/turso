--
-- pg_get_* catalog functions, quote_ident/quote_literal, format_type,
-- acldefault
--
-- Converted from pg/head/tests/pg_get_functions.rs. Recorded from real
-- PostgreSQL 18 (see README.md); never edited by hand. Names carry an
-- fg_ prefix so they cannot collide with another file's fixtures in the
-- shared schedule database.
--

-- quote_ident matches PostgreSQL 18
SELECT quote_ident('users');
SELECT quote_ident('Users');
SELECT quote_ident('select');
SELECT quote_ident('has space');

-- quote_literal quotes and passes NULL through
SELECT quote_literal('it''s');
SELECT quote_literal(NULL::text);

-- format_type names the declared column types and falls back for the rest
SELECT format_type(23::oid, -1);
SELECT format_type(20::oid, -1);
SELECT format_type(25::oid, -1);
SELECT format_type(19::oid, -1);
SELECT format_type(18::oid, -1);
SELECT format_type(999999::oid, -1);

-- acldefault renders a table and a schema default ACL as text
SELECT acldefault('r', 10::oid);
SELECT acldefault('n', 10::oid);

-- pg_proc.proacl is NULL for a built-in function with no explicit grant
SELECT proacl FROM pg_proc WHERE proname = 'set_config';

-- acldefault accepts an explicit char cast like pg_dump sends
SELECT acldefault('r'::"char", 10::oid);
SELECT acldefault(CASE WHEN true THEN 'n'::"char" ELSE 'r'::"char" END, 10::oid);

CREATE TABLE fg_t (id integer PRIMARY KEY);

-- pg_get_constraintdef and pg_get_indexdef describe a primary key; a
-- PRIMARY KEY column gets its own NOT NULL constraint row too, allocated
-- (like PostgreSQL's own) before the primary key's
SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid = 'fg_t'::regclass ORDER BY oid;
SELECT pg_get_indexdef(indexrelid) FROM pg_index WHERE indrelid = 'fg_t'::regclass;
SELECT pg_get_constraintdef(999999::oid);
SELECT pg_get_constraintdef(oid, false) FROM pg_constraint WHERE conrelid = 'fg_t'::regclass ORDER BY oid;
SELECT pg_get_indexdef(999999::oid);

-- pg_get_indexdef of a column past the last key is an empty string
SELECT pg_get_indexdef(indexrelid, 1, true) FROM pg_index WHERE indrelid = 'fg_t'::regclass;
SELECT pg_get_indexdef(indexrelid, 2, true) FROM pg_index WHERE indrelid = 'fg_t'::regclass;

-- pg_get_expr decompiles a policy's USING expression
CREATE POLICY p ON fg_t FOR SELECT USING (id > 0);
SELECT pg_get_expr(polqual, polrelid) FROM pg_policy WHERE polrelid = 'fg_t'::regclass;
SELECT pg_get_expr(polwithcheck, polrelid) FROM pg_policy WHERE polrelid = 'fg_t'::regclass;

-- A policy using every expression kind the grammar admits (comparisons,
-- boolean connectives, IS NULL/TRUE/FALSE/UNKNOWN, IS DISTINCT FROM (both
-- polarities), concatenation, current_user, a cast, a cast directly over a
-- literal, a simple and a searched CASE, and integer and text literals)
-- round-trips through storage: pg_get_expr matches PostgreSQL 18's own
-- text for the same USING clause (a cast over a literal folds to the one
-- constant it names, with no residual cast, the same as PostgreSQL's own
-- analyzer), and the policy still filters rows correctly once re-admitted.
CREATE TABLE fg_t2 (id integer, name text, tag text);
INSERT INTO fg_t2 VALUES (7, 'n1', NULL), (8, 'n2', NULL), (7, NULL, NULL);
CREATE ROLE fg_bob LOGIN;
GRANT SELECT ON fg_t2 TO fg_bob;
CREATE POLICY q ON fg_t2 FOR SELECT USING (
    (id > 0) AND (id < 1000) AND (id <= 100) AND (id >= 0) AND (id <> -5)
    AND (id = 7) AND (name IS NOT NULL) AND (tag IS NULL) AND ((id > 0) IS TRUE)
    AND ((id > 0) IS NOT FALSE) AND ((id < 0) IS FALSE) AND ((id < 0) IS NOT TRUE)
    AND ((tag = 'q') IS UNKNOWN) AND ((id > 0) IS NOT UNKNOWN)
    AND (tag IS DISTINCT FROM 'y') AND ((tag || 'z') IS NULL)
    AND (current_user = 'bob') AND (name IS NULL OR NOT (id = 0))
    AND (id::text = '7')
    AND ((CASE id WHEN 7 THEN 1 ELSE 0 END) = 1)
    AND ((CASE WHEN id = 7 THEN 1 ELSE 0 END) = 1)
    AND ('a' IS NOT DISTINCT FROM 'a')
    AND (NULL::integer IS NULL)
    AND (('5'::integer) = 5)
);
ALTER TABLE fg_t2 ENABLE ROW LEVEL SECURITY;
SELECT pg_get_expr(polqual, polrelid) FROM pg_policy WHERE polrelid = 'fg_t2'::regclass;
\c - fg_bob
SELECT id FROM fg_t2;
\c - postgres
