--
-- Rows: INSERT and SELECT over ordinary tables
--
-- Converted from pg/head/tests/rows.rs. Recorded from real PostgreSQL 18
-- (see README.md); never edited by hand. Names carry an r_ prefix so they
-- cannot collide with another file's fixtures in the shared schedule
-- database. Where the original Rust test recreated the same table fresh
-- per test, this file instead threads one persistent r_notes table
-- through every scenario, so every id used below is unique across the
-- whole file.
--

CREATE TABLE r_notes (id integer PRIMARY KEY, body text NOT NULL, views bigint);

-- the owner inserts rows (a multi-row VALUES, and a column-list insert
-- reordering id after body) and reads them back
INSERT INTO r_notes VALUES (1, 'first', 9223372036854775807), (2, 'second', NULL);
INSERT INTO r_notes (body, id) VALUES ('third', '3');
SELECT * FROM r_notes;
SELECT body FROM r_notes WHERE id = 1 OR (id = '3' AND body = 'third');

-- a duplicate key names the primary key constraint, and leaves the
-- existing row unchanged
INSERT INTO r_notes VALUES (1, 'again', NULL);

-- inserts and selects PostgreSQL itself rejects, with an undefined table,
-- an undefined column and an out-of-range integer that carry no other
-- side effect on r_notes.
INSERT INTO nope VALUES (1);
INSERT INTO r_notes VALUES (2147483648, 'a', 1);
SELECT zzz_absent FROM r_notes;
SELECT * FROM r_notes WHERE zzz_absent = 1;

-- an undefined target column in an INSERT's own column list
INSERT INTO r_notes (id, nope) VALUES (70, 'a');

-- an INSERT with more expressions than target columns (a position at the
-- extra literal's own node)
INSERT INTO r_notes VALUES (60, 'a', 1, 2);

-- an INSERT with more target columns than expressions (a position at the
-- extra target column's own name)
INSERT INTO r_notes (id, body, views) VALUES (71);

-- an undefined operator, positioned at the operator itself
SELECT * FROM r_notes WHERE id = body;

-- an invalid integer text representation in a WHERE comparison
SELECT * FROM r_notes WHERE id = 'abc';

-- a failed insert leaves no rows behind: the second row of this statement
-- violates NOT NULL, so neither new id appears afterward
INSERT INTO r_notes VALUES (10, 'kept?', 1), (11, NULL, 1);
SELECT id FROM r_notes WHERE id IN (10, 11);

-- a multi-row insert checks each row in order, like PostgreSQL: the
-- duplicate key on the first new row is reported and nothing from either
-- row is kept
INSERT INTO r_notes VALUES (20, 'first', NULL);
INSERT INTO r_notes VALUES (20, 'dup key', NULL), (21, NULL, NULL);
SELECT * FROM r_notes WHERE id IN (20, 21);

-- a duplicate key within a single multi-row insert is reported and
-- nothing is kept
INSERT INTO r_notes VALUES (30, 'first', NULL), (30, 'second', NULL);
SELECT id FROM r_notes WHERE id = 30;

-- a NOT NULL violation earlier in a multi-row insert is reported before a
-- later duplicate key, and the existing row is unaffected
INSERT INTO r_notes VALUES (40, 'first', NULL);
INSERT INTO r_notes VALUES (41, NULL, NULL), (40, 'dup key', NULL);
SELECT * FROM r_notes WHERE id IN (40, 41);

-- a NULL primary key is refused even though the engine would assign one
INSERT INTO r_notes VALUES (NULL, 'x', NULL);
INSERT INTO r_notes (body) VALUES ('x');

-- quoted integers follow PostgreSQL 18's own input rules: underscores as
-- digit group separators, 0x/0X hex, 0o octal, 0b binary, surrounding
-- whitespace, a unary plus, a negative hex literal, and the int4 minimum.
-- Each round-trips: the same text that inserted the row also selects it
-- back.
INSERT INTO r_notes VALUES ('1_000_000', 'x', NULL);
SELECT id FROM r_notes WHERE id = '1_000_000';
INSERT INTO r_notes VALUES ('0x_100000', 'x', NULL);
SELECT id FROM r_notes WHERE id = '0x_100000';
INSERT INTO r_notes VALUES ('0X200000', 'x', NULL);
SELECT id FROM r_notes WHERE id = '0X200000';
INSERT INTO r_notes VALUES ('0o4000000', 'x', NULL);
SELECT id FROM r_notes WHERE id = '0o4000000';
INSERT INTO r_notes VALUES ('0b100000000000000000000', 'x', NULL);
SELECT id FROM r_notes WHERE id = '0b100000000000000000000';
INSERT INTO r_notes VALUES (' 1000006 ', 'x', NULL);
SELECT id FROM r_notes WHERE id = ' 1000006 ';
INSERT INTO r_notes VALUES ('+1000007', 'x', NULL);
SELECT id FROM r_notes WHERE id = '+1000007';
INSERT INTO r_notes VALUES ('-0x100002', 'x', NULL);
SELECT id FROM r_notes WHERE id = '-0x100002';
INSERT INTO r_notes VALUES ('-2147483648', 'x', NULL);
SELECT id FROM r_notes WHERE id = '-2147483648';

-- every malformed shape of that same text: a leading, trailing or doubled
-- underscore, a bare "0x", an empty or blank string, "+-5", trailing
-- garbage, "1e3", "1.0"
INSERT INTO r_notes VALUES ('_1', 'x', NULL);
INSERT INTO r_notes VALUES ('1_', 'x', NULL);
INSERT INTO r_notes VALUES ('1__0', 'x', NULL);
INSERT INTO r_notes VALUES ('0x_', 'x', NULL);
INSERT INTO r_notes VALUES ('0x', 'x', NULL);
INSERT INTO r_notes VALUES ('', 'x', NULL);
INSERT INTO r_notes VALUES (' ', 'x', NULL);
INSERT INTO r_notes VALUES ('+-5', 'x', NULL);
INSERT INTO r_notes VALUES ('5 x', 'x', NULL);
INSERT INTO r_notes VALUES ('1e3', 'x', NULL);
INSERT INTO r_notes VALUES ('1.0', 'x', NULL);

-- three out-of-range text lookups, and a bigint-out-of-range insert
SELECT id FROM r_notes WHERE id = '2147483648';
SELECT id FROM r_notes WHERE id = '0x80000000';
SELECT id FROM r_notes WHERE id = '9223372036854775808';
INSERT INTO r_notes VALUES (90, 'x', '9223372036854775808');

-- quoted names that differ only by case stay apart: DML on one never
-- touches the other
CREATE TABLE "R_Case" ("Body" text, body text);
CREATE TABLE r_case (body text);
INSERT INTO "R_Case" VALUES ('upper', 'lower');
INSERT INTO r_case VALUES ('other table');
SELECT "Body" FROM "R_Case";
SELECT body FROM r_case;

-- a select-list alias renames the output column PostgreSQL-style,
-- including the default "?column?" for an unaliased expression
INSERT INTO r_notes VALUES (50, 'hello', 3);
SELECT body AS b, views AS "Views", body || '!' FROM r_notes WHERE id = 50;

-- a NOT NULL violation's "Failing row contains (...)" DETAIL is not
-- record_out: every value renders unquoted and verbatim, even an empty
-- string, or one with a comma, quote, backslash, space or parentheses;
-- only NULL becomes the bare word null. b is left NULL to trigger the
-- violation while a bigint or the interesting text sits in c or b.
CREATE TABLE r_detail (id integer PRIMARY KEY, a text NOT NULL, b text, c bigint);
INSERT INTO r_detail (id, b) VALUES (1, '');
INSERT INTO r_detail (id, b) VALUES (2, 'a,b');
INSERT INTO r_detail (id, b) VALUES (3, 'a"b');
INSERT INTO r_detail (id, b) VALUES (4, 'a\b');
INSERT INTO r_detail (id, b) VALUES (5, 'a b');
INSERT INTO r_detail (id, b) VALUES (6, '(a)');
INSERT INTO r_detail (id, c) VALUES (7, 9223372036854775807);
