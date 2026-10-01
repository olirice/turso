--
-- Sessions: settings, transactions, locking, row security opt-out
--
-- Converted from pg/head/tests/session.rs (with several refused-by-name
-- cases in refusals.sql). Recorded from real PostgreSQL 18 (see
-- README.md); never edited by hand. Names carry an sn_ prefix so they
-- cannot collide with another file's fixtures in the shared schedule
-- database.
--

CREATE ROLE sn_bob LOGIN;
CREATE TABLE sn_notes (id integer PRIMARY KEY, owner text);
INSERT INTO sn_notes VALUES (1, 'bob'), (2, 'carol');
GRANT SELECT ON sn_notes TO sn_bob;
CREATE TABLE sn_hidden (id integer);

-- settings are accepted only at values that do not change behavior
SET DATESTYLE = ISO;
SET INTERVALSTYLE = POSTGRES;
SET extra_float_digits TO 3;
SET synchronize_seqscans TO off;
SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET row_security = off;
SET nosuch = 1;

-- an empty search path hides unqualified names
SELECT pg_catalog.set_config('search_path', '', false);
SELECT id FROM sn_notes;
CREATE TABLE sn_other (id integer);
SELECT id FROM public.sn_notes;
SET search_path = public;
SELECT id FROM sn_notes;

-- a read-only transaction refuses writes, and a failed one refuses
-- everything but a syntax error
BEGIN;
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY;
SELECT id FROM sn_notes;
INSERT INTO sn_notes VALUES (3, 'x');
SELECT id FROM sn_notes;
COMMIT;
SELECT id FROM sn_notes;

-- a read-only transaction refuses every write statement
BEGIN READ ONLY;
CREATE TABLE sn_t (a integer);
ROLLBACK;
BEGIN READ ONLY;
GRANT SELECT ON sn_notes TO sn_bob;
ROLLBACK;
BEGIN READ ONLY;
CREATE ROLE sn_carol LOGIN;
ROLLBACK;
BEGIN READ ONLY;
CREATE POLICY p ON sn_notes FOR SELECT USING (id = 1);
ROLLBACK;
BEGIN READ ONLY;
ALTER TABLE sn_notes ENABLE ROW LEVEL SECURITY;
ROLLBACK;

-- SET TRANSACTION READ WRITE after a query is too late
BEGIN READ ONLY;
SELECT id FROM sn_notes;
SET TRANSACTION READ WRITE;
ROLLBACK;

-- the isolation level must be set before any query
BEGIN;
SELECT id FROM sn_notes;
SET TRANSACTION READ ONLY;
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ;
ROLLBACK;

-- a rolled-back transaction leaves nothing, and a committed one is visible
BEGIN;
INSERT INTO sn_notes VALUES (3, 'x');
ROLLBACK;
\c
SELECT id FROM sn_notes;
BEGIN;
INSERT INTO sn_notes VALUES (3, 'x');
COMMIT;
\c
SELECT id FROM sn_notes;

-- a setting changed in a rolled-back transaction is restored
BEGIN;
SET search_path = '';
ROLLBACK;
SELECT id FROM sn_notes;

-- LOCK TABLE needs a transaction and a privilege
\c - sn_bob
LOCK TABLE sn_hidden IN ACCESS SHARE MODE;
BEGIN;
LOCK TABLE public.sn_notes IN ACCESS SHARE MODE;
LOCK TABLE sn_hidden IN ACCESS SHARE MODE;
ROLLBACK;

-- LOCK TABLE does not count as the transaction's first query
BEGIN;
LOCK TABLE sn_notes IN ACCESS SHARE MODE;
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ;
ROLLBACK;

\c - postgres

-- in a failed transaction, only a syntax error (kept in session.rs: see
-- README.md's known gaps on bare syntax errors having no position) comes
-- before the aborted-transaction error
BEGIN;
SELECT id FROM sn_nosuch_rel;
DROP TABLE sn_notes;
ROLLBACK;

-- SET LOCAL and set_config(..., true) revert at both COMMIT and ROLLBACK
SET search_path = 'base';
BEGIN;
SET LOCAL search_path = 'local';
SELECT current_setting('search_path');
COMMIT;
SELECT current_setting('search_path');
BEGIN;
SET LOCAL search_path = 'local';
SELECT current_setting('search_path');
ROLLBACK;
SELECT current_setting('search_path');
BEGIN;
SELECT set_config('search_path', 'local2', true);
SELECT current_setting('search_path');
COMMIT;
SELECT current_setting('search_path');
BEGIN;
SELECT set_config('search_path', 'local2', true);
SELECT current_setting('search_path');
ROLLBACK;
SELECT current_setting('search_path');

-- current_setting of an unknown setting errors, and missing_ok returns NULL
SELECT current_setting('nosuch');
SELECT current_setting('nosuch', true);

-- an unregistered function is refused
SELECT no_such_function(1);

SET search_path = public;

-- a session function with a non-constant argument is evaluated per row
CREATE TABLE sn_setting_names (name text);
INSERT INTO sn_setting_names VALUES ('search_path'), ('row_security');
SELECT current_setting(name) FROM sn_setting_names;

-- set_config over rows applies every row in order
CREATE TABLE sn_greetings (value text);
INSERT INTO sn_greetings VALUES ('first'), ('second');
SELECT set_config('search_path', value, false) FROM sn_greetings;
SELECT current_setting('search_path');

-- two sessions see each other's committed DDL
CREATE TABLE sn_shared_ddl (id integer);
\c
SELECT relname FROM pg_class WHERE relname = 'sn_shared_ddl';
