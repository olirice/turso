--
-- PREPARE, EXECUTE, DEALLOCATE
--
-- Converted from pg/head/tests/prepare_execute.rs (one case stays there:
-- see README.md). Recorded from real PostgreSQL 18; never edited by hand.
-- Names carry a pe_ prefix so they cannot collide with another file's
-- fixtures in the shared schedule database.
--

CREATE TABLE pe_notes (id integer PRIMARY KEY, body text);
INSERT INTO pe_notes VALUES (1, 'first'), (2, 'second');

-- PREPARE then EXECUTE returns rows, once per argument
PREPARE pe_getbyid(integer) AS SELECT body FROM pe_notes WHERE id = $1;
EXECUTE pe_getbyid(1);
EXECUTE pe_getbyid(2);

-- EXECUTE coerces a quoted literal to the declared parameter type
EXECUTE pe_getbyid('1');

-- EXECUTE with a badly typed argument errors like PostgreSQL, at the
-- argument's own position
EXECUTE pe_getbyid('abc');

-- EXECUTE of an unknown name errors
EXECUTE pe_nosuch;

-- a duplicate PREPARE errors
PREPARE pe_p1 AS SELECT 1;
PREPARE pe_p1 AS SELECT 2;

-- PREPARE and EXECUTE fold the name like PostgreSQL
PREPARE pe_MyPlan AS SELECT 1;
EXECUTE pe_myplan;

-- a quoted PREPARE name keeps its case and is not reached by the folded form
PREPARE "pe_MyPlan2" AS SELECT 1;
EXECUTE pe_myplan2;
EXECUTE "pe_MyPlan2";

-- a prepared statement does not survive a new connection
CREATE ROLE pe_alice LOGIN;
\c - pe_alice
PREPARE pe_getbyid2(integer) AS SELECT body FROM pe_notes WHERE id = $1;
\c - pe_alice
EXECUTE pe_getbyid2(1);

-- a prepared statement is not transactional, so it survives a rollback
BEGIN;
PREPARE pe_ptx AS SELECT 1;
ROLLBACK;
EXECUTE pe_ptx;
