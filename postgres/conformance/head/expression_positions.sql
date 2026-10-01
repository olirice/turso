--
-- Expression position restrictions
--
-- Converted from pg/head/tests/expression_positions.rs (the remaining cases
-- PostgreSQL itself admits but the head does not implement stay in
-- refusals.sql; see README.md). Recorded from real PostgreSQL 18; never
-- edited by hand. Names carry an ep_ prefix so they cannot collide with
-- another file's fixtures in the shared schedule database.
--

CREATE TABLE ep_t (a integer PRIMARY KEY, b integer);
CREATE TABLE ep_t2 (x integer PRIMARY KEY);
INSERT INTO ep_t VALUES (1, 10), (2, 20);
INSERT INTO ep_t2 VALUES (1), (2);

-- WHERE: a subquery is allowed; an aggregate and a set-returning function
-- are refused by PostgreSQL's own placement rule
SELECT * FROM ep_t WHERE (SELECT true);
SELECT * FROM ep_t WHERE array_agg(a) IS NOT NULL;
SELECT * FROM ep_t WHERE generate_series(1, 2) > 0;

-- JOIN/ON: same shape as WHERE, PostgreSQL's own phrasing differs
SELECT * FROM ep_t JOIN ep_t2 ON (SELECT true) ORDER BY a, x;
SELECT * FROM ep_t JOIN ep_t2 ON array_agg(a) IS NOT NULL;
SELECT * FROM ep_t JOIN ep_t2 ON generate_series(1, 2) > 0;

-- SELECT target: PostgreSQL allows a subquery, an aggregate and a
-- catalog-rendered function (scoped to ep_t's own constraint, so the head's
-- reduced catalog matches PostgreSQL's row for row)
SELECT (SELECT 1) FROM ep_t;
SELECT array_agg(a) FROM ep_t;
SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid = 'ep_t'::regclass ORDER BY oid;

-- ORDER BY: PostgreSQL allows a subquery folded into the target list
SELECT a FROM ep_t ORDER BY (SELECT 1);

-- a table function's own argument list: PostgreSQL allows a subquery,
-- refuses an aggregate by its own rule, and refuses a nested set-returning
-- function ("must appear at top level of FROM")
SELECT * FROM generate_series(1, (SELECT 2));
SELECT * FROM generate_series(1, array_agg(1));
SELECT * FROM generate_series(1, generate_series(1, 2));

-- INSERT ... VALUES: PostgreSQL refuses an aggregate by its own rule
INSERT INTO ep_t (a) VALUES (array_agg(1));

-- an EXECUTE argument: PostgreSQL itself refuses a subquery, an aggregate
-- and a set-returning function, each by its own rule
PREPARE ep_p1(int) AS SELECT $1;
EXECUTE ep_p1((SELECT 1));
PREPARE ep_p2(int) AS SELECT $1;
EXECUTE ep_p2(array_agg(1));
PREPARE ep_p4(int) AS SELECT $1;
EXECUTE ep_p4(generate_series(1, 2));

-- a policy's USING expression: PostgreSQL refuses an aggregate and a
-- set-returning function, each by its own rule
CREATE POLICY ep_agg ON ep_t FOR SELECT USING (array_agg(a) IS NOT NULL);
CREATE POLICY ep_srf ON ep_t FOR SELECT USING (generate_series(1, 2) > 0);

-- an array literal cast's own invalid-input error: WHERE gets a LINE
-- position, a policy's USING does not, the same split as every case above
SELECT * FROM ep_t WHERE ('{1,2,x}'::int4[]) IS NOT NULL;
CREATE POLICY ep_arr ON ep_t FOR SELECT USING (('{1,2,x}'::int4[]) IS NOT NULL);

-- a parameter reference with no PREPARE to bind it has no parameter to
-- bind to, the same WHERE/policy split as every case above
SELECT * FROM ep_t WHERE a = $1;
CREATE POLICY ep_param ON ep_t FOR SELECT USING (a = $1);
