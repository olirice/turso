--
-- Expressions: every kind admitted in a row-security policy's USING clause
--
-- Converted from pg/head/tests/expressions.rs. Recorded from real
-- PostgreSQL 18 (see README.md); never edited by hand. Names carry an
-- ex_ prefix. Every fragment below is a constant, always-true boolean
-- expression (its point is proving the expression kind is admitted,
-- parsed, stored, spliced into the policy and evaluated correctly, the
-- same one traversal every other expression context already exercised
-- elsewhere in this corpus goes through); each policy's own table isolates
-- it from every other feature's policy, so ex_bob (granted SELECT on all
-- of them, never their owner, so row security actually applies to him)
-- sees the row in every case.
--

CREATE ROLE ex_bob LOGIN;

CREATE TABLE ex_t01 (id integer PRIMARY KEY);
INSERT INTO ex_t01 VALUES (1);
GRANT SELECT ON ex_t01 TO ex_bob;
CREATE POLICY p ON ex_t01 FOR SELECT USING (5 = 5);
ALTER TABLE ex_t01 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t02 (id integer PRIMARY KEY);
INSERT INTO ex_t02 VALUES (1);
GRANT SELECT ON ex_t02 TO ex_bob;
CREATE POLICY p ON ex_t02 FOR SELECT USING (5 <> 6);
ALTER TABLE ex_t02 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t03 (id integer PRIMARY KEY);
INSERT INTO ex_t03 VALUES (1);
GRANT SELECT ON ex_t03 TO ex_bob;
CREATE POLICY p ON ex_t03 FOR SELECT USING (5 < 6);
ALTER TABLE ex_t03 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t04 (id integer PRIMARY KEY);
INSERT INTO ex_t04 VALUES (1);
GRANT SELECT ON ex_t04 TO ex_bob;
CREATE POLICY p ON ex_t04 FOR SELECT USING (6 > 5);
ALTER TABLE ex_t04 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t05 (id integer PRIMARY KEY);
INSERT INTO ex_t05 VALUES (1);
GRANT SELECT ON ex_t05 TO ex_bob;
CREATE POLICY p ON ex_t05 FOR SELECT USING (5 <= 5);
ALTER TABLE ex_t05 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t06 (id integer PRIMARY KEY);
INSERT INTO ex_t06 VALUES (1);
GRANT SELECT ON ex_t06 TO ex_bob;
CREATE POLICY p ON ex_t06 FOR SELECT USING (5 >= 5);
ALTER TABLE ex_t06 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t07 (id integer PRIMARY KEY);
INSERT INTO ex_t07 VALUES (1);
GRANT SELECT ON ex_t07 TO ex_bob;
CREATE POLICY p ON ex_t07 FOR SELECT USING ((5 = 5) AND (6 = 6));
ALTER TABLE ex_t07 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t08 (id integer PRIMARY KEY);
INSERT INTO ex_t08 VALUES (1);
GRANT SELECT ON ex_t08 TO ex_bob;
CREATE POLICY p ON ex_t08 FOR SELECT USING ((5 = 6) OR (6 = 6));
ALTER TABLE ex_t08 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t09 (id integer PRIMARY KEY);
INSERT INTO ex_t09 VALUES (1);
GRANT SELECT ON ex_t09 TO ex_bob;
CREATE POLICY p ON ex_t09 FOR SELECT USING (NOT (5 = 6));
ALTER TABLE ex_t09 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t10 (id integer PRIMARY KEY);
INSERT INTO ex_t10 VALUES (1);
GRANT SELECT ON ex_t10 TO ex_bob;
CREATE POLICY p ON ex_t10 FOR SELECT USING (NULL::integer IS NULL);
ALTER TABLE ex_t10 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t11 (id integer PRIMARY KEY);
INSERT INTO ex_t11 VALUES (1);
GRANT SELECT ON ex_t11 TO ex_bob;
CREATE POLICY p ON ex_t11 FOR SELECT USING (5 IS NOT NULL);
ALTER TABLE ex_t11 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t12 (id integer PRIMARY KEY);
INSERT INTO ex_t12 VALUES (1);
GRANT SELECT ON ex_t12 TO ex_bob;
CREATE POLICY p ON ex_t12 FOR SELECT USING ((5 = 5) IS TRUE);
ALTER TABLE ex_t12 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t13 (id integer PRIMARY KEY);
INSERT INTO ex_t13 VALUES (1);
GRANT SELECT ON ex_t13 TO ex_bob;
CREATE POLICY p ON ex_t13 FOR SELECT USING ((5 = 6) IS NOT TRUE);
ALTER TABLE ex_t13 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t14 (id integer PRIMARY KEY);
INSERT INTO ex_t14 VALUES (1);
GRANT SELECT ON ex_t14 TO ex_bob;
CREATE POLICY p ON ex_t14 FOR SELECT USING ((5 = 6) IS FALSE);
ALTER TABLE ex_t14 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t15 (id integer PRIMARY KEY);
INSERT INTO ex_t15 VALUES (1);
GRANT SELECT ON ex_t15 TO ex_bob;
CREATE POLICY p ON ex_t15 FOR SELECT USING ((5 = 5) IS NOT FALSE);
ALTER TABLE ex_t15 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t16 (id integer PRIMARY KEY);
INSERT INTO ex_t16 VALUES (1);
GRANT SELECT ON ex_t16 TO ex_bob;
CREATE POLICY p ON ex_t16 FOR SELECT USING ((NULL::integer = 5) IS UNKNOWN);
ALTER TABLE ex_t16 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t17 (id integer PRIMARY KEY);
INSERT INTO ex_t17 VALUES (1);
GRANT SELECT ON ex_t17 TO ex_bob;
CREATE POLICY p ON ex_t17 FOR SELECT USING ((5 = 5) IS NOT UNKNOWN);
ALTER TABLE ex_t17 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t18 (id integer PRIMARY KEY);
INSERT INTO ex_t18 VALUES (1);
GRANT SELECT ON ex_t18 TO ex_bob;
CREATE POLICY p ON ex_t18 FOR SELECT USING (5 IS DISTINCT FROM 6);
ALTER TABLE ex_t18 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t19 (id integer PRIMARY KEY);
INSERT INTO ex_t19 VALUES (1);
GRANT SELECT ON ex_t19 TO ex_bob;
CREATE POLICY p ON ex_t19 FOR SELECT USING (5 IS NOT DISTINCT FROM 5);
ALTER TABLE ex_t19 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t20 (id integer PRIMARY KEY);
INSERT INTO ex_t20 VALUES (1);
GRANT SELECT ON ex_t20 TO ex_bob;
CREATE POLICY p ON ex_t20 FOR SELECT USING (('a' || 'b') = 'ab');
ALTER TABLE ex_t20 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t21 (id integer PRIMARY KEY);
INSERT INTO ex_t21 VALUES (1);
GRANT SELECT ON ex_t21 TO ex_bob;
CREATE POLICY p ON ex_t21 FOR SELECT USING ((CASE 5 WHEN 5 THEN 1 ELSE 0 END) = 1);
ALTER TABLE ex_t21 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t22 (id integer PRIMARY KEY);
INSERT INTO ex_t22 VALUES (1);
GRANT SELECT ON ex_t22 TO ex_bob;
CREATE POLICY p ON ex_t22 FOR SELECT USING ((CASE WHEN 5 = 5 THEN 1 ELSE 0 END) = 1);
ALTER TABLE ex_t22 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t23 (id integer PRIMARY KEY);
INSERT INTO ex_t23 VALUES (1);
GRANT SELECT ON ex_t23 TO ex_bob;
CREATE POLICY p ON ex_t23 FOR SELECT USING (('5'::integer) = 5);
ALTER TABLE ex_t23 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t24 (id integer PRIMARY KEY);
INSERT INTO ex_t24 VALUES (1);
GRANT SELECT ON ex_t24 TO ex_bob;
CREATE POLICY p ON ex_t24 FOR SELECT USING (((5 = 5)));
ALTER TABLE ex_t24 ENABLE ROW LEVEL SECURITY;

-- A bare integer literal explicitly cast to a different type never folds
-- (unlike a text literal's cast): PostgreSQL 18 keeps the original
-- integer constant and the cast both, so `pg_get_expr` shows
-- `(174358563)::text`, never `'174358563'::text`.
CREATE TABLE ex_t25 (id integer PRIMARY KEY);
INSERT INTO ex_t25 VALUES (1);
GRANT SELECT ON ex_t25 TO ex_bob;
CREATE POLICY p ON ex_t25 FOR SELECT USING ((174358563)::text = (174358563)::text);
ALTER TABLE ex_t25 ENABLE ROW LEVEL SECURITY;

-- A cast whose target already equals its argument's own type is elided
-- entirely, for any argument shape, not only a literal.
CREATE TABLE ex_t26 (id integer PRIMARY KEY);
INSERT INTO ex_t26 VALUES (1);
GRANT SELECT ON ex_t26 TO ex_bob;
CREATE POLICY p ON ex_t26 FOR SELECT USING ((('5'::integer))::integer = 5);
ALTER TABLE ex_t26 ENABLE ROW LEVEL SECURITY;

-- A CASE arm that is a bare integer literal, unified against another
-- arm's wider numeric type, keeps its own explicit cast the same way
-- (`('5'::integer)::bigint`-shaped, never silently retyped in place).
CREATE TABLE ex_t27 (id integer PRIMARY KEY);
INSERT INTO ex_t27 VALUES (1);
GRANT SELECT ON ex_t27 TO ex_bob;
CREATE POLICY p ON ex_t27 FOR SELECT USING ((CASE WHEN 1 = 1 THEN 5 ELSE 6::bigint END) = 5);
ALTER TABLE ex_t27 ENABLE ROW LEVEL SECURITY;

-- A CASE nested inside an enclosing CASE's own WHEN condition: PostgreSQL
-- 18's pretty-printer indents the whole nested CASE one level deeper than
-- its parent.
CREATE TABLE ex_t28 (id integer PRIMARY KEY);
INSERT INTO ex_t28 VALUES (1);
GRANT SELECT ON ex_t28 TO ex_bob;
CREATE POLICY p ON ex_t28 FOR SELECT USING ((CASE WHEN (1 = CASE WHEN 1 = 1 THEN 1 ELSE 2 END) THEN 5 ELSE 6 END) = 5);
ALTER TABLE ex_t28 ENABLE ROW LEVEL SECURITY;

-- `IS [NOT] DISTINCT FROM` against a bare, uncast NULL token rewrites to
-- `IS [NOT] NULL` at the parse edge, the same as PostgreSQL 18 itself
-- (an explicit `NULL::type` does not qualify; ex_t18/ex_t19 already cover
-- the ordinary, non-rewritten form).
CREATE TABLE ex_t29 (id integer PRIMARY KEY);
INSERT INTO ex_t29 VALUES (1);
GRANT SELECT ON ex_t29 TO ex_bob;
CREATE POLICY p ON ex_t29 FOR SELECT USING (5 IS DISTINCT FROM NULL);
ALTER TABLE ex_t29 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t30 (id integer PRIMARY KEY);
INSERT INTO ex_t30 VALUES (1);
GRANT SELECT ON ex_t30 TO ex_bob;
CREATE POLICY p ON ex_t30 FOR SELECT USING (NOT (5 IS NOT DISTINCT FROM NULL));
ALTER TABLE ex_t30 ENABLE ROW LEVEL SECURITY;

-- A CASE with no arm of a fixed (column) type anchors on the widest
-- natural type any bare integer arm needs, `bigint` once one overflows
-- `integer`, never a fixed `integer` default regardless of magnitude.
CREATE TABLE ex_t31 (id integer PRIMARY KEY);
INSERT INTO ex_t31 VALUES (1);
GRANT SELECT ON ex_t31 TO ex_bob;
CREATE POLICY p ON ex_t31 FOR SELECT USING ((CASE WHEN 1 = 1 THEN -2147483649 ELSE NULL END) = -2147483649);
ALTER TABLE ex_t31 ENABLE ROW LEVEL SECURITY;

-- A CASE arm's own fixed type can still change the anchor arrived at so
-- far when the two are a different, but mutually coercible, type: the
-- comparison operator between `text` and `name` never needs a cast (a
-- direct cross-type operator, checked below), but CASE's own unification
-- has no such operator, so PostgreSQL 18 always casts one arm into the
-- other, and it is whichever arm comes later that wins (probed: swapping
-- current_user (name) and a text arm between THEN and ELSE swaps which
-- one grows the cast).
CREATE TABLE ex_t32 (id integer PRIMARY KEY);
INSERT INTO ex_t32 VALUES (1);
GRANT SELECT ON ex_t32 TO ex_bob;
CREATE POLICY p ON ex_t32 FOR SELECT USING ((CASE WHEN 1 = 1 THEN current_user ELSE 'z'::text END) = current_user);
ALTER TABLE ex_t32 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t33 (id integer PRIMARY KEY);
INSERT INTO ex_t33 VALUES (1);
GRANT SELECT ON ex_t33 TO ex_bob;
CREATE POLICY p ON ex_t33 FOR SELECT USING ((CASE WHEN 1 = 1 THEN 'z'::text ELSE current_user END) = current_user);
ALTER TABLE ex_t33 ENABLE ROW LEVEL SECURITY;

-- The same widening across a CASE's own arms, for the two numeric column
-- types this head declares (`integer`/`bigint`): whichever arm is
-- `bigint` always wins the anchor, regardless of position (only
-- `integer -> bigint` is ever a valid implicit cast, so PostgreSQL 18 has
-- no choice either way). These two reference their own row's columns
-- (unlike every other policy in this file, a constant expression), the
-- only way to put two differently-typed declarable columns in one CASE.
CREATE TABLE ex_t34 (id integer PRIMARY KEY, n bigint NOT NULL);
INSERT INTO ex_t34 VALUES (5, 5);
GRANT SELECT ON ex_t34 TO ex_bob;
CREATE POLICY p ON ex_t34 FOR SELECT USING (n = CASE WHEN 1 = 1 THEN id ELSE n END);
ALTER TABLE ex_t34 ENABLE ROW LEVEL SECURITY;

CREATE TABLE ex_t35 (id integer PRIMARY KEY, n bigint NOT NULL);
INSERT INTO ex_t35 VALUES (5, 5);
GRANT SELECT ON ex_t35 TO ex_bob;
CREATE POLICY p ON ex_t35 FOR SELECT USING (n = CASE WHEN 1 = 1 THEN n ELSE id END);
ALTER TABLE ex_t35 ENABLE ROW LEVEL SECURITY;

-- A CASE arm that is a bare integer literal is concretely typed at parse
-- time, not PostgreSQL's polymorphic "unknown": mismatched against
-- another arm of an incompatible category, PostgreSQL 18 raises the same
-- "CASE types ... cannot be matched" (42804) two mismatched columns
-- would, naming the arm that broke the match first and the running
-- candidate second (probed). This CREATE POLICY itself fails, so
-- ex_t36 keeps row security enabled with no policy at all: ex_bob's own
-- SELECT below returns no rows, PostgreSQL's own default deny.
CREATE TABLE ex_t36 (id integer PRIMARY KEY, x text NOT NULL);
INSERT INTO ex_t36 VALUES (1, 'a');
GRANT SELECT ON ex_t36 TO ex_bob;
ALTER TABLE ex_t36 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p ON ex_t36 FOR SELECT USING ((CASE WHEN true THEN 5 ELSE x END) = x);

-- A CASE arm typed regclass and one typed regproc share PostgreSQL's own
-- numeric typcategory, but neither casts to the other implicitly: each
-- casts to oid, never to the other directly (probed, pg_cast). PostgreSQL
-- 18 raises a different error than a genuine cross-category mismatch,
-- "CASE/WHEN could not convert type ... to ..." (42846), naming the arm
-- processed so far first and the one that could not join it second. This
-- CREATE POLICY itself fails, so ex_t37 keeps row security enabled with
-- no policy at all.
CREATE TABLE ex_t37 (id integer PRIMARY KEY);
INSERT INTO ex_t37 VALUES (1);
GRANT SELECT ON ex_t37 TO ex_bob;
ALTER TABLE ex_t37 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p ON ex_t37 FOR SELECT USING ((CASE WHEN true THEN 5::regclass ELSE 6::regproc END) = 5::regclass);

-- current_user does not get its own policy here: PostgreSQL 18's own row
-- security implementation refuses a policy qual that reduces to no table
-- column reference at all ("cannot cope with variable-free clause"), even
-- though the identical comparison is fine in an ordinary WHERE clause
-- (checked below). current_user's admission as an expression is proven
-- there instead.

\c - ex_bob
SELECT id FROM ex_t01;
SELECT id FROM ex_t02;
SELECT id FROM ex_t03;
SELECT id FROM ex_t04;
SELECT id FROM ex_t05;
SELECT id FROM ex_t06;
SELECT id FROM ex_t07;
SELECT id FROM ex_t08;
SELECT id FROM ex_t09;
SELECT id FROM ex_t10;
SELECT id FROM ex_t11;
SELECT id FROM ex_t12;
SELECT id FROM ex_t13;
SELECT id FROM ex_t14;
SELECT id FROM ex_t15;
SELECT id FROM ex_t16;
SELECT id FROM ex_t17;
SELECT id FROM ex_t18;
SELECT id FROM ex_t19;
SELECT id FROM ex_t20;
SELECT id FROM ex_t21;
SELECT id FROM ex_t22;
SELECT id FROM ex_t23;
SELECT id FROM ex_t24;
SELECT id FROM ex_t25;
SELECT id FROM ex_t26;
SELECT id FROM ex_t27;
SELECT id FROM ex_t28;
SELECT id FROM ex_t29;
SELECT id FROM ex_t30;
SELECT id FROM ex_t31;
SELECT id FROM ex_t32;
SELECT id FROM ex_t33;
SELECT id FROM ex_t34;
SELECT id FROM ex_t35;
SELECT id FROM ex_t36;
SELECT id FROM ex_t37;

-- current_user is admitted as an ordinary expression (see the note above
-- on why it does not get its own policy)
SELECT id FROM ex_t01 WHERE current_user = current_user;

--
-- INSERT VALUES admits only a literal or a cast of a literal
--
\c - postgres
CREATE TABLE ex_scratch (id integer PRIMARY KEY, flag text);
INSERT INTO ex_scratch VALUES (1, 'a');
INSERT INTO ex_scratch VALUES (2, '3'::text);
INSERT INTO ex_scratch VALUES ('4'::integer, NULL);
SELECT id, flag FROM ex_scratch ORDER BY id;
