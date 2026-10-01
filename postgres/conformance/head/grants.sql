--
-- GRANT and role privileges
--
-- Converted from pg/head/tests/grants.rs. Recorded from real PostgreSQL 18
-- (see README.md); never edited by hand. Names carry a gr_ prefix so they
-- cannot collide with another file's fixtures in the shared schedule
-- database. Each behavior below uses its own roles and table so an earlier
-- GRANT cannot change a later assertion's starting privileges.
--

-- a role logs in only with LOGIN
CREATE ROLE gr_alice LOGIN;
CREATE ROLE gr_carol;
CREATE ROLE gr_dave NOLOGIN;
\c - gr_alice
\c - postgres

-- CREATE ROLE checks permission, then reserved names, then existence
CREATE ROLE gr_bob LOGIN;
\c - gr_alice
CREATE ROLE gr_bob;
\c - postgres
CREATE ROLE pg_x;
CREATE ROLE gr_alice;

-- a role without a grant is denied every statement
CREATE TABLE gr_notes1 (id integer PRIMARY KEY, body text);
INSERT INTO gr_notes1 VALUES (1, 'hello');
\c - gr_bob
SELECT * FROM gr_notes1;
INSERT INTO gr_notes1 VALUES (2, 'no');
\c - postgres

-- each granted privilege allows exactly its statement
CREATE TABLE gr_notes2 (id integer PRIMARY KEY, body text);
INSERT INTO gr_notes2 VALUES (1, 'hello');
GRANT INSERT ON gr_notes2 TO gr_bob;
\c - gr_bob
INSERT INTO gr_notes2 VALUES (2, 'from bob');
SELECT * FROM gr_notes2;
\c - postgres
GRANT SELECT ON TABLE gr_notes2 TO gr_bob;
GRANT SELECT ON TABLE gr_notes2 TO gr_bob;
\c - gr_bob
SELECT id FROM gr_notes2 ORDER BY id;
\c - postgres

-- a grant to PUBLIC reaches every role
CREATE TABLE gr_notes3 (id integer PRIMARY KEY, body text);
INSERT INTO gr_notes3 VALUES (1, 'hello');
GRANT SELECT ON gr_notes3 TO PUBLIC;
\c - gr_alice
SELECT id FROM gr_notes3 ORDER BY id;
\c - gr_bob
SELECT id FROM gr_notes3 ORDER BY id;
\c - postgres

-- every table and schema privilege can be granted
CREATE TABLE gr_notes4 (id integer PRIMARY KEY, body text);
GRANT ALL ON gr_notes4 TO gr_alice;
GRANT UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN ON gr_notes4 TO gr_bob;
GRANT USAGE ON SCHEMA public TO gr_bob;
GRANT ALL ON SCHEMA public TO gr_alice;
\c - gr_alice
CREATE TABLE gr_diary4 (entry text);
\c - postgres

-- grants resolve objects, then roles, then privilege types
CREATE TABLE gr_notes5 (id integer PRIMARY KEY, body text);
GRANT USAGE ON gr_nosuch5 TO gr_nobody5;
GRANT USAGE ON gr_notes5 TO gr_nobody5;
GRANT USAGE ON gr_notes5 TO gr_bob;
GRANT SELECT ON SCHEMA public TO gr_bob;
GRANT USAGE ON SCHEMA gr_nosuch5 TO gr_bob;
GRANT SELECT ON gr_notes5, gr_nosuch5 TO gr_bob;

-- only the owner grants
CREATE TABLE gr_notes6 (id integer PRIMARY KEY, body text);
\c - gr_bob
GRANT SELECT ON gr_notes6 TO gr_bob;
\c - postgres

-- creating a table needs CREATE on public, and that check comes first
CREATE ROLE gr_erin LOGIN;
\c - gr_erin
CREATE TABLE g8(a int PRIMARY KEY,a int PRIMARY KEY);
\c - postgres
GRANT CREATE ON SCHEMA public TO gr_erin;
\c - gr_erin
CREATE TABLE gr_diary8 (entry text);
INSERT INTO gr_diary8 VALUES ('mine');
GRANT SELECT ON gr_diary8 TO gr_bob;
\c - gr_bob
SELECT entry FROM gr_diary8;
\c - postgres
