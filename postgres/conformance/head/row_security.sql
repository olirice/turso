--
-- Row-level security
--
-- Converted from pg/head/tests/row_security.rs. Recorded from real
-- PostgreSQL 18 (see README.md); never edited by hand. Names carry an rs_
-- prefix so they cannot collide with another file's fixtures in the shared
-- schedule database.
--

CREATE ROLE rs_alice LOGIN;
CREATE ROLE rs_bob LOGIN;
CREATE ROLE rs_carol LOGIN;
GRANT CREATE ON SCHEMA public TO rs_alice;

\c - rs_alice
CREATE TABLE rs_notes (id integer PRIMARY KEY, owner text, body text NOT NULL);
INSERT INTO rs_notes VALUES (1, 'rs_alice', 'a'), (2, 'rs_bob', 'b'), (3, NULL, 'n'), (4, 'rs_carol', 'c');
GRANT SELECT, INSERT ON rs_notes TO rs_bob, rs_carol;
CREATE POLICY own ON rs_notes FOR SELECT TO rs_bob USING (owner = current_user);
\c - postgres

-- a policy has no effect until row security is enabled
\c - rs_bob
SELECT id FROM rs_notes ORDER BY id;
\c - rs_alice
ALTER TABLE rs_notes ENABLE ROW LEVEL SECURITY;
\c - rs_bob
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- a role with a grant but no policy sees nothing
\c - rs_carol
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- permissive policies are combined with OR
\c - rs_alice
CREATE POLICY fourth ON rs_notes FOR SELECT TO rs_bob USING (id = 4);
\c - rs_bob
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- a policy for PUBLIC applies to every role
\c - rs_alice
CREATE POLICY first ON rs_notes FOR SELECT USING (id = 1);
\c - rs_carol
SELECT id FROM rs_notes ORDER BY id;
\c - rs_bob
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- a WHERE clause cannot widen what a policy allows
\c - rs_bob
SELECT id FROM rs_notes WHERE id = 1 OR id = id ORDER BY id;
\c - postgres

-- a NULL policy result hides the row
\c - rs_alice
CREATE POLICY nothing ON rs_notes FOR SELECT TO rs_carol USING (owner = NULL);
\c - rs_carol
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- the owner bypasses row security until it is forced, and a superuser always does
\c - rs_alice
SELECT id FROM rs_notes ORDER BY id;
ALTER TABLE rs_notes FORCE ROW LEVEL SECURITY;
SELECT id FROM rs_notes ORDER BY id;
\c - postgres
SELECT id FROM rs_notes ORDER BY id;
\c - rs_alice
ALTER TABLE rs_notes NO FORCE ROW LEVEL SECURITY, DISABLE ROW LEVEL SECURITY;
\c - rs_carol
SELECT id FROM rs_notes ORDER BY id;
\c - postgres

-- an INSERT under row security is refused before NOT NULL is checked
\c - rs_alice
ALTER TABLE rs_notes ENABLE ROW LEVEL SECURITY;
\c - rs_bob
INSERT INTO rs_notes VALUES (5, 'rs_bob', 'x');
INSERT INTO rs_notes VALUES (5, 'rs_bob', NULL);
\c - rs_alice
INSERT INTO rs_notes VALUES (5, 'rs_alice', 'x');
ALTER TABLE rs_notes FORCE ROW LEVEL SECURITY;
INSERT INTO rs_notes VALUES (6, 'rs_alice', 'x');
\c - postgres

-- current_user is the session role with its case
CREATE ROLE "Rs_Bob" LOGIN;
\c - rs_alice
GRANT SELECT ON rs_notes TO "Rs_Bob";
INSERT INTO rs_notes VALUES (7, 'Rs_Bob', 'upper');
CREATE POLICY mine ON rs_notes FOR SELECT TO "Rs_Bob" USING (owner = current_user);
\c - "Rs_Bob"
SELECT id FROM rs_notes ORDER BY id;
\c - rs_bob
SELECT body FROM rs_notes WHERE owner = current_user;
\c - postgres

-- CREATE POLICY errors come in PostgreSQL's order
\c - rs_bob
CREATE POLICY p ON rs_nosuch FOR SELECT USING (id = 1) WITH CHECK (id = 1);
CREATE POLICY p ON rs_nosuch FOR SELECT TO rs_nobody USING (id = 1);
CREATE POLICY p ON rs_notes FOR SELECT TO rs_nobody USING (id = 1);
\c - rs_alice
CREATE POLICY own2 ON rs_notes FOR SELECT TO rs_nobody USING (nope = 1);
CREATE POLICY own2 ON rs_notes FOR SELECT USING (nope = 1);
CREATE POLICY p2 ON rs_notes FOR SELECT USING (nope = 1);
CREATE POLICY p2 ON rs_notes FOR SELECT USING (id = 'x');
CREATE POLICY p2 ON rs_notes FOR SELECT USING (id = current_user);
CREATE POLICY p2 ON rs_notes FOR SELECT USING (current_user = 1);
\c - postgres

-- only the owner changes row security
\c - rs_bob
ALTER TABLE rs_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE rs_notes FORCE ROW LEVEL SECURITY;
ALTER TABLE rs_notes NO FORCE ROW LEVEL SECURITY;
\c - postgres

-- a scalar subquery and EXISTS need SELECT on the inner table
CREATE ROLE rs_dave LOGIN;
GRANT CREATE ON SCHEMA public TO rs_dave;
\c - rs_dave
CREATE TABLE rs_hidden (id integer PRIMARY KEY);
CREATE TABLE rs_visible (id integer PRIMARY KEY);
INSERT INTO rs_hidden VALUES (1);
INSERT INTO rs_visible VALUES (1);
GRANT SELECT ON rs_visible TO rs_bob;
\c - rs_bob
SELECT (SELECT id FROM rs_hidden);
SELECT id FROM rs_visible WHERE EXISTS (SELECT 1 FROM rs_hidden);
SELECT id FROM rs_visible WHERE id IN (SELECT id FROM rs_hidden);
\c - postgres

-- a subquery over a row-security table agrees with a top-level read
CREATE ROLE rs_erin LOGIN;
GRANT CREATE ON SCHEMA public TO rs_erin;
\c - rs_erin
CREATE TABLE rs_sub (id integer PRIMARY KEY, owner text);
INSERT INTO rs_sub VALUES (1, 'rs_alice'), (2, 'rs_bob');
GRANT SELECT ON rs_sub TO rs_bob;
CREATE POLICY own3 ON rs_sub FOR SELECT TO rs_bob USING (owner = current_user);
ALTER TABLE rs_sub ENABLE ROW LEVEL SECURITY;
\c - rs_bob
SELECT id FROM rs_sub ORDER BY id;
SELECT id FROM rs_sub WHERE id IN (SELECT id FROM rs_sub) ORDER BY id;
\c - postgres

-- CROSS JOIN and a derived table both respect row security
CREATE ROLE rs_frank LOGIN;
GRANT CREATE ON SCHEMA public TO rs_frank;
\c - rs_frank
CREATE TABLE rs_depts (id integer PRIMARY KEY, name text);
INSERT INTO rs_depts VALUES (1, 'eng'), (2, 'sales');
CREATE TABLE rs_join_notes (id integer PRIMARY KEY, dept_id integer, owner text);
INSERT INTO rs_join_notes VALUES (1, 1, 'rs_alice'), (2, 1, 'rs_bob'), (3, 2, 'rs_alice');
GRANT SELECT ON rs_depts, rs_join_notes TO rs_bob;
CREATE POLICY own4 ON rs_join_notes FOR SELECT TO rs_bob USING (owner = current_user);
ALTER TABLE rs_join_notes ENABLE ROW LEVEL SECURITY;
\c - rs_bob
SELECT d.id, n.id FROM rs_depts d CROSS JOIN rs_join_notes n ORDER BY d.id, n.id;
SELECT id, owner FROM rs_join_notes ORDER BY id;
SELECT id, owner FROM (SELECT * FROM rs_join_notes) AS n ORDER BY id;
SELECT d.name, n.id FROM rs_depts d JOIN (SELECT * FROM rs_join_notes) n ON d.id = n.dept_id ORDER BY d.name;
\c - postgres
