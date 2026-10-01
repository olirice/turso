--
-- pg_roles, pg_settings, pg_seclabels
--
-- Converted from pg/head/tests/views.rs: the three constant views pg_dump
-- reads. Recorded from real PostgreSQL 18 (see README.md); never edited by
-- hand.
--

-- the three views appear in pg_class with relkind 'v'
SELECT relname, relkind FROM pg_class WHERE relname IN ('pg_roles', 'pg_settings', 'pg_seclabels') ORDER BY relname;

-- pg_seclabels has no rows but the columns pg_dump reads all exist
SELECT objoid, classoid, objsubid, objtype, objnamespace, objname, provider, label FROM pg_seclabels;

-- pg_roles lists created roles
CREATE ROLE v_alice LOGIN;
CREATE ROLE v_carol LOGIN;
SELECT rolname FROM pg_roles WHERE rolname IN ('v_alice', 'v_carol') ORDER BY rolname;

-- a non-superuser reads a masked rolpassword through pg_roles, and is
-- refused direct access to pg_authid
\c - v_alice
SELECT rolname, rolpassword FROM pg_roles WHERE rolname = 'v_alice';
SELECT rolname FROM pg_authid;

-- a SET setting is read back from pg_settings and agrees with current_setting
\c - postgres
SET extra_float_digits = 3;
SELECT setting FROM pg_settings WHERE name = 'extra_float_digits';
SELECT current_setting('extra_float_digits');

-- pg_settings preserves PostgreSQL's own mixed-case names verbatim
SELECT name FROM pg_settings WHERE name IN ('DateStyle', 'IntervalStyle') ORDER BY name;

-- writes to a view are refused the same way PostgreSQL itself refuses them
INSERT INTO pg_roles (rolname) VALUES ('mallory');
