--
-- SELECT: joins, set operations, aggregates, table functions
--
-- Converted from pg/head/tests/queries.rs. Recorded from real PostgreSQL 18
-- (see README.md); never edited by hand. Table and role names carry a q_
-- prefix so they cannot collide with another file's fixtures in the shared
-- schedule database.
--

CREATE TABLE q_departments (id integer PRIMARY KEY, name text);
CREATE TABLE q_employees (id integer PRIMARY KEY, dept_id integer, name text);
INSERT INTO q_departments VALUES (1, 'eng'), (2, 'sales');
INSERT INTO q_employees VALUES (1, 1, 'alice'), (2, 1, 'bob'), (3, NULL, 'carol');

-- inner join with ON
SELECT e.name, d.name FROM q_employees e JOIN q_departments d ON e.dept_id = d.id ORDER BY e.name;

-- left join keeps the unmatched row
SELECT e.name, d.name FROM q_employees e LEFT JOIN q_departments d ON e.dept_id = d.id ORDER BY e.name;

-- a join between a user table and a pg_catalog relation
SELECT e.name FROM q_employees e JOIN pg_class c ON c.relname = 'q_employees' ORDER BY e.name;

-- table aliases and alias-qualified star
SELECT e.* FROM q_employees e WHERE e.name = 'alice';

-- a correlated subquery
SELECT d.name FROM q_departments d WHERE EXISTS (SELECT 1 FROM q_employees e WHERE e.dept_id = d.id) ORDER BY d.name;

-- UNION ALL keeps duplicates (each department name appears twice)
SELECT name FROM q_departments UNION ALL SELECT name FROM q_departments ORDER BY name;

-- DISTINCT removes duplicate rows
SELECT DISTINCT dept_id FROM q_employees ORDER BY dept_id;

-- ORDER BY default nulls ordering (nulls last ascending, first descending)
SELECT name FROM q_employees ORDER BY dept_id ASC, name;
SELECT name FROM q_employees ORDER BY dept_id DESC, name;

-- ambiguous and missing column errors. Kept short (a two-letter table pair,
-- not q_employees/q_departments): psql's own LINE excerpt clips a longer
-- statement with "...", which the runner does not yet emulate; see
-- head/README.md.
CREATE TABLE q_a (id integer);
CREATE TABLE q_b (id integer);
SELECT id FROM q_a x JOIN q_b y ON x.id = y.id;
SELECT zzz_absent FROM q_a x JOIN q_b y ON x.id = y.id;
SELECT z.id FROM q_employees e;

-- a SELECT with no FROM is a query over an empty FROM
SELECT 'x';

-- CROSS JOIN is the cartesian product
SELECT e.id, d.id FROM q_employees e CROSS JOIN q_departments d ORDER BY e.id, d.id;

-- comma-separated FROM is the cartesian product
SELECT e.id, d.id FROM q_employees e, q_departments d ORDER BY e.id, d.id;

-- comma-separated FROM with three tables chains left to right (3 x 2 x 3 = 18 rows)
SELECT e.id, d.id, e2.id FROM q_employees e, q_departments d, q_employees e2 ORDER BY e.id, d.id, e2.id;

-- a set-returning function as one of a comma-separated FROM list
SELECT s, d.name FROM generate_series(1, 2) s, q_departments d WHERE s = d.id ORDER BY s;

-- array_agg with ORDER BY sorts the aggregated values
SELECT (SELECT array_agg(id ORDER BY id DESC) FROM q_employees);
SELECT (SELECT array_agg(id ORDER BY id) FROM q_employees WHERE dept_id = 1);

-- ORDER BY an output column alias
SELECT name AS n FROM q_employees ORDER BY n DESC;

-- a derived table can wrap a UNION ALL
SELECT id FROM (SELECT id FROM q_departments WHERE id = 1 UNION ALL SELECT id FROM q_departments WHERE id = 2) AS u ORDER BY id;

-- a derived table with column aliases renames its columns
SELECT n FROM (SELECT id FROM q_departments WHERE id = 1) AS d(n);

CREATE TABLE q_employees_sal (id integer PRIMARY KEY, dept_id integer, name text, salary integer);
INSERT INTO q_employees_sal VALUES (1, 1, 'alice', 100), (2, 1, 'bob', 200), (3, NULL, 'carol', 300);

-- array_agg over rows matches the table
SELECT array_agg(id) FROM q_employees_sal;

-- array_agg over zero rows is NULL, not an empty array
SELECT array_agg(id) FROM q_employees_sal WHERE false;

-- an aggregate in WHERE is refused with a position at its own call
SELECT array_agg(id) FROM q_employees_sal WHERE array_agg(id) IS NOT NULL;

-- mixing an aggregate with an ungrouped column is refused, positioned at
-- the ungrouped column itself
SELECT array_agg(id), name FROM q_employees_sal;

-- array_agg inside a correlated scalar subquery aggregates per outer row
SELECT (SELECT array_agg(e2.salary) FROM q_employees_sal e2 WHERE e2.dept_id = e1.dept_id)
FROM q_employees_sal e1 ORDER BY e1.id;

-- array_remove drops every matching element
SELECT array_remove('{a,b,a,c}'::text[], 'a');
SELECT array_remove(array_remove('{a,b,c}'::text[], 'a'), 'b');

-- array_to_string joins elements with the delimiter
SELECT array_to_string('{a,b,c}'::text[], ', ');
SELECT array_to_string('{}'::text[], ', ');

CREATE TABLE q_things_bare (id integer PRIMARY KEY);

-- array_remove and array_to_string of a NULL array (an unset reloptions
-- column) are NULL, and compose over the same catalog column
SELECT array_remove(reloptions, 'a') FROM pg_class WHERE relname = 'q_things_bare';
SELECT array_to_string(reloptions, ', ') FROM pg_class WHERE relname = 'q_things_bare';
SELECT array_to_string(array_remove(reloptions, 'x'), ', ') FROM pg_class WHERE relname = 'q_things_bare';

-- array_agg with ORDER BY over zero rows is NULL, not an empty array
SELECT (SELECT array_agg(id ORDER BY id) FROM q_things_bare WHERE id = 1);

CREATE TABLE q_ordering_probe (a integer, b text);
INSERT INTO q_ordering_probe VALUES (2, 'b'), (1, 'a');

-- ORDER BY a column position sorts by that output column
SELECT a, b FROM q_ordering_probe ORDER BY 1;

-- ORDER BY a position outside the select list, and position zero
SELECT 1 AS a, 2 AS b ORDER BY 5;
SELECT 1 AS a, 2 AS b ORDER BY 0;

-- ORDER BY on a UNION accepts a result column position or a result column name
SELECT 1 AS a, 2 AS b UNION ALL SELECT 3, 4 UNION ALL SELECT 1, 2 ORDER BY 1, 2;
SELECT 2 AS a UNION ALL SELECT 1 ORDER BY a;

-- ORDER BY on a UNION refuses a column that is not in the select list
SELECT 1 AS a UNION ALL SELECT 2 ORDER BY zzz_absent;

-- array literal casts round-trip
SELECT '{1,2}'::int2[];
SELECT '{16384,16385}'::oid[];
SELECT '{a,b}'::text[];

-- an out-of-range array literal element
SELECT '{1,70000}'::int2[];
SELECT '{-70000}'::int2[];

-- a smallint array cast of an int2vector column keeps its zero-based indexing
SELECT (indkey::int2[])[0], (indkey::int2[])[1] FROM pg_index WHERE indexrelid = 2662;
SELECT array_upper(indkey::int2[], 1) FROM pg_index WHERE indexrelid = 2662;
SELECT array_upper(indkey, 1) FROM pg_index WHERE indexrelid = 2662;

-- unnest over a literal array expands into rows, and over an empty one is empty
SELECT tbloid FROM unnest('{16386,16393,16400}'::pg_catalog.oid[]) AS src(tbloid) ORDER BY tbloid;
SELECT tbloid FROM unnest('{}'::pg_catalog.oid[]) AS src(tbloid);

CREATE TABLE q_things_named (id integer PRIMARY KEY, name text);
INSERT INTO q_things_named VALUES (1, 'a'), (2, 'b'), (3, 'c');

-- unnest over a literal array can join to a real table
SELECT t.name FROM unnest('{1,3}'::pg_catalog.oid[]) AS src(tid)
JOIN q_things_named t ON t.id = src.tid ORDER BY t.name;

-- unnest over an array column matches PostgreSQL on a column with no options set
SELECT v FROM unnest((SELECT spcoptions FROM pg_tablespace WHERE spcname = 'pg_default')) AS x(v);

-- a table function without a matching pg_proc row is refused by name
SELECT * FROM no_such_table_function(1) AS x(v);

-- pg_options_to_table splits key=value pairs, and is empty over an empty array
SELECT option_name, option_value FROM pg_catalog.pg_options_to_table('{a=1,b=2}'::text[]) ORDER BY option_name;
SELECT option_name FROM pg_catalog.pg_options_to_table('{}'::text[]);

-- generate_series expands an inclusive integer range, is empty when start is
-- after stop, and names its own column when the query gives it no alias
SELECT * FROM generate_series(1, 4);
SELECT * FROM generate_series(4, 1);
SELECT generate_series FROM generate_series(1, 1);
SELECT s FROM generate_series(1, 1) s;

-- a table function argument correlated to an enclosing query is allowed,
-- with or without its own ORDER BY
SELECT a.attname, array_to_string(ARRAY(SELECT s FROM generate_series(0, a.attnum::integer) s), ',')
FROM pg_attribute a WHERE a.attrelid = 'q_things_named'::regclass AND a.attnum > 0 ORDER BY a.attnum;
-- (attnum > 0 excludes PostgreSQL's system columns, which the head's
-- pg_attribute does not model; see the report for this batch)
SELECT a.attname, array_to_string(ARRAY(SELECT option_name FROM pg_options_to_table(a.attfdwoptions) ORDER BY option_name), ', ')
FROM pg_attribute a WHERE a.attrelid = 'q_things_bare'::regclass AND a.attnum > 0 ORDER BY a.attnum;
