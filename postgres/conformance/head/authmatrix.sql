--
-- Authorization and row-security matrix
--
-- The 100 of 370 PostgreSQL-recorded privilege and row-security
-- scenarios the head can run, written out once from
-- pg/head/tests/authmatrix/cells.rs, deleted by the commit "pg: move
-- grants, row security, authmatrix and prepared statements to the
-- corpus" (its parent has it). The other 270
-- are counted by reason, with a probe each, in refusals.sql.
-- Table, role and policy names are content hashes of each scenario's
-- own name, so they cannot collide with another file's fixtures or with
-- each other.
--

CREATE ROLE matrix_owner LOGIN;
CREATE ROLE matrix_stranger LOGIN;
CREATE ROLE matrix_grantee LOGIN;
GRANT CREATE ON SCHEMA public TO matrix_owner;
-- create_role/ok/superuser/plain/bare
\c - postgres
CREATE ROLE rt_02815190;
-- create_role/ok/stranger/plain/bare
\c - matrix_stranger
CREATE ROLE rt_82a81f62;
-- create_role/exists/superuser/plain/bare
\c - postgres
CREATE ROLE rt_5f2942b6;
CREATE ROLE rt_5f2942b6;
-- create_role/exists/stranger/plain/bare
\c - postgres
CREATE ROLE rt_41d5f56a;
\c - matrix_stranger
CREATE ROLE rt_41d5f56a;
-- grant_on_table/table_absent/owner/plain/bare
\c - matrix_owner
GRANT SELECT ON t_42bcacaa TO matrix_grantee;
-- grant_on_table/table_absent/stranger/plain/bare
\c - matrix_stranger
GRANT SELECT ON t_dc5b33cf TO matrix_grantee;
-- grant_on_table/table_absent/superuser/plain/bare
\c - postgres
GRANT SELECT ON t_dfc9e867 TO matrix_grantee;
-- grant_on_table/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_c3aa27dc (id integer PRIMARY KEY, c integer);
GRANT SELECT ON public.t_c3aa27dc TO matrix_grantee;
-- grant_on_table/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_6d5764dd (id integer PRIMARY KEY, c integer);
GRANT SELECT ON t_6d5764dd TO matrix_grantee;
-- grant_on_table/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_91e2a905 (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
GRANT SELECT ON public.t_91e2a905 TO matrix_grantee;
-- grant_on_table/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_5c910aa1 (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
GRANT SELECT ON t_5c910aa1 TO matrix_grantee;
-- grant_on_table/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_d42e9094 (id integer PRIMARY KEY, c integer);
\c - postgres
GRANT SELECT ON public.t_d42e9094 TO matrix_grantee;
-- grant_on_table/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_c7958a33 (id integer PRIMARY KEY, c integer);
\c - postgres
GRANT SELECT ON t_c7958a33 TO matrix_grantee;
-- create_policy/table_absent/owner/plain/bare
\c - matrix_owner
CREATE POLICY p_59676a28 ON t_59676a28 FOR SELECT USING (id = 1);
-- create_policy/table_absent/stranger/plain/bare
\c - matrix_stranger
CREATE POLICY p_d4b74360 ON t_d4b74360 FOR SELECT USING (id = 1);
-- create_policy/table_absent/superuser/plain/bare
\c - postgres
CREATE POLICY p_7384e796 ON t_7384e796 FOR SELECT USING (id = 1);
-- create_policy/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_eb616649 (id integer PRIMARY KEY, c integer);
CREATE POLICY p_eb616649 ON public.t_eb616649 FOR SELECT USING (id = 1);
-- create_policy/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_e21454cc (id integer PRIMARY KEY, c integer);
CREATE POLICY p_e21454cc ON t_e21454cc FOR SELECT USING (id = 1);
-- create_policy/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_f99235dc (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
CREATE POLICY p_f99235dc ON public.t_f99235dc FOR SELECT USING (id = 1);
-- create_policy/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_eae09dde (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
CREATE POLICY p_eae09dde ON t_eae09dde FOR SELECT USING (id = 1);
-- create_policy/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_42bf2da0 (id integer PRIMARY KEY, c integer);
\c - postgres
CREATE POLICY p_42bf2da0 ON public.t_42bf2da0 FOR SELECT USING (id = 1);
-- create_policy/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_d4610c3e (id integer PRIMARY KEY, c integer);
\c - postgres
CREATE POLICY p_d4610c3e ON t_d4610c3e FOR SELECT USING (id = 1);
-- create_policy/policy_exists/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_43c4c14f (id integer PRIMARY KEY, c integer);
CREATE POLICY p_43c4c14f ON t_43c4c14f FOR SELECT USING (id = 1);
CREATE POLICY p_43c4c14f ON public.t_43c4c14f FOR SELECT USING (id = 1);
-- create_policy/policy_exists/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_2ada91fe (id integer PRIMARY KEY, c integer);
CREATE POLICY p_2ada91fe ON t_2ada91fe FOR SELECT USING (id = 1);
CREATE POLICY p_2ada91fe ON t_2ada91fe FOR SELECT USING (id = 1);
-- create_policy/policy_exists/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_a192f88d (id integer PRIMARY KEY, c integer);
CREATE POLICY p_a192f88d ON t_a192f88d FOR SELECT USING (id = 1);
\c - matrix_stranger
CREATE POLICY p_a192f88d ON public.t_a192f88d FOR SELECT USING (id = 1);
-- create_policy/policy_exists/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_f0f28ed1 (id integer PRIMARY KEY, c integer);
CREATE POLICY p_f0f28ed1 ON t_f0f28ed1 FOR SELECT USING (id = 1);
\c - matrix_stranger
CREATE POLICY p_f0f28ed1 ON t_f0f28ed1 FOR SELECT USING (id = 1);
-- create_policy/policy_exists/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_c777a22c (id integer PRIMARY KEY, c integer);
CREATE POLICY p_c777a22c ON t_c777a22c FOR SELECT USING (id = 1);
\c - postgres
CREATE POLICY p_c777a22c ON public.t_c777a22c FOR SELECT USING (id = 1);
-- create_policy/policy_exists/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_020d9e37 (id integer PRIMARY KEY, c integer);
CREATE POLICY p_020d9e37 ON t_020d9e37 FOR SELECT USING (id = 1);
\c - postgres
CREATE POLICY p_020d9e37 ON t_020d9e37 FOR SELECT USING (id = 1);
-- alter_table_enable_rls/table_absent/owner/plain/bare
\c - matrix_owner
ALTER TABLE t_57237492 ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/table_absent/stranger/plain/bare
\c - matrix_stranger
ALTER TABLE t_574d9a3d ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/table_absent/superuser/plain/bare
\c - postgres
ALTER TABLE t_2d91b56d ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_e5907d3e (id integer PRIMARY KEY, c integer);
ALTER TABLE public.t_e5907d3e ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_bd78af44 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_bd78af44 ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_96af337a (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
ALTER TABLE public.t_96af337a ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_2616fbf8 (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
ALTER TABLE t_2616fbf8 ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_a686111c (id integer PRIMARY KEY, c integer);
\c - postgres
ALTER TABLE public.t_a686111c ENABLE ROW LEVEL SECURITY;
-- alter_table_enable_rls/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_29df05ce (id integer PRIMARY KEY, c integer);
\c - postgres
ALTER TABLE t_29df05ce ENABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/table_absent/owner/plain/bare
\c - matrix_owner
ALTER TABLE t_bf151008 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/table_absent/stranger/plain/bare
\c - matrix_stranger
ALTER TABLE t_387eff86 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/table_absent/superuser/plain/bare
\c - postgres
ALTER TABLE t_6ac4faa9 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_24d242e9 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_24d242e9 ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.t_24d242e9 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_7f7efd2d (id integer PRIMARY KEY, c integer);
ALTER TABLE t_7f7efd2d ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_7f7efd2d DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_1705e821 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_1705e821 ENABLE ROW LEVEL SECURITY;
\c - matrix_stranger
ALTER TABLE public.t_1705e821 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_dcda865f (id integer PRIMARY KEY, c integer);
ALTER TABLE t_dcda865f ENABLE ROW LEVEL SECURITY;
\c - matrix_stranger
ALTER TABLE t_dcda865f DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_9e693713 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_9e693713 ENABLE ROW LEVEL SECURITY;
\c - postgres
ALTER TABLE public.t_9e693713 DISABLE ROW LEVEL SECURITY;
-- alter_table_disable_rls/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_4c728b28 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_4c728b28 ENABLE ROW LEVEL SECURITY;
\c - postgres
ALTER TABLE t_4c728b28 DISABLE ROW LEVEL SECURITY;
-- alter_table_force_rls/table_absent/owner/plain/bare
\c - matrix_owner
ALTER TABLE t_e53fd8cb FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/table_absent/stranger/plain/bare
\c - matrix_stranger
ALTER TABLE t_7cf36e96 FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/table_absent/superuser/plain/bare
\c - postgres
ALTER TABLE t_9d29da5c FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_7e9ed27c (id integer PRIMARY KEY, c integer);
ALTER TABLE public.t_7e9ed27c FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_a5cad5a9 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_a5cad5a9 FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_6a556707 (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
ALTER TABLE public.t_6a556707 FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_b6d6529e (id integer PRIMARY KEY, c integer);
\c - matrix_stranger
ALTER TABLE t_b6d6529e FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_8e3ba0f4 (id integer PRIMARY KEY, c integer);
\c - postgres
ALTER TABLE public.t_8e3ba0f4 FORCE ROW LEVEL SECURITY;
-- alter_table_force_rls/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_d71dacb2 (id integer PRIMARY KEY, c integer);
\c - postgres
ALTER TABLE t_d71dacb2 FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/table_absent/owner/plain/bare
\c - matrix_owner
ALTER TABLE t_74c8f88a NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/table_absent/stranger/plain/bare
\c - matrix_stranger
ALTER TABLE t_4f1d4969 NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/table_absent/superuser/plain/bare
\c - postgres
ALTER TABLE t_3a23c188 NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/owner/plain/qualified
\c - matrix_owner
CREATE TABLE t_02c39288 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_02c39288 FORCE ROW LEVEL SECURITY;
ALTER TABLE public.t_02c39288 NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_103f35ca (id integer PRIMARY KEY, c integer);
ALTER TABLE t_103f35ca FORCE ROW LEVEL SECURITY;
ALTER TABLE t_103f35ca NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/stranger/plain/qualified
\c - matrix_owner
CREATE TABLE t_197ac9d5 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_197ac9d5 FORCE ROW LEVEL SECURITY;
\c - matrix_stranger
ALTER TABLE public.t_197ac9d5 NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/stranger/plain/bare
\c - matrix_owner
CREATE TABLE t_c0a6e342 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_c0a6e342 FORCE ROW LEVEL SECURITY;
\c - matrix_stranger
ALTER TABLE t_c0a6e342 NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/superuser/plain/qualified
\c - matrix_owner
CREATE TABLE t_dad2df9f (id integer PRIMARY KEY, c integer);
ALTER TABLE t_dad2df9f FORCE ROW LEVEL SECURITY;
\c - postgres
ALTER TABLE public.t_dad2df9f NO FORCE ROW LEVEL SECURITY;
-- alter_table_no_force_rls/ok/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_844ea190 (id integer PRIMARY KEY, c integer);
ALTER TABLE t_844ea190 FORCE ROW LEVEL SECURITY;
\c - postgres
ALTER TABLE t_844ea190 NO FORCE ROW LEVEL SECURITY;
-- rls_select/enabled/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_b9969445 (id integer PRIMARY KEY, c integer);
INSERT INTO t_b9969445 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_b9969445 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_b9969445 ON t_b9969445 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_b9969445 TO matrix_grantee;
\c - postgres
SELECT id FROM t_b9969445;
-- rls_select/enabled/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_2d3a1e21 (id integer PRIMARY KEY, c integer);
INSERT INTO t_2d3a1e21 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_2d3a1e21 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_2d3a1e21 ON t_2d3a1e21 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_2d3a1e21 TO matrix_grantee;
SELECT id FROM t_2d3a1e21;
-- rls_select/enabled/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_2853b950 (id integer PRIMARY KEY, c integer);
INSERT INTO t_2853b950 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_2853b950 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_2853b950 ON t_2853b950 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_2853b950 TO matrix_grantee;
\c - matrix_grantee
SELECT id FROM t_2853b950;
-- rls_select/enabled_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_7a41079b (id integer PRIMARY KEY, c integer);
INSERT INTO t_7a41079b VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_7a41079b ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_7a41079b ON t_7a41079b FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_7a41079b TO matrix_grantee;
\c - postgres
SET row_security = off;
SELECT id FROM t_7a41079b;
-- rls_select/enabled_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_9ea76b61 (id integer PRIMARY KEY, c integer);
INSERT INTO t_9ea76b61 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_9ea76b61 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_9ea76b61 ON t_9ea76b61 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_9ea76b61 TO matrix_grantee;
SET row_security = off;
SELECT id FROM t_9ea76b61;
-- rls_select/enabled_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_8a39540f (id integer PRIMARY KEY, c integer);
INSERT INTO t_8a39540f VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_8a39540f ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_8a39540f ON t_8a39540f FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_8a39540f TO matrix_grantee;
\c - matrix_grantee
SET row_security = off;
SELECT id FROM t_8a39540f;
-- rls_select/forced/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_d35c5cae (id integer PRIMARY KEY, c integer);
INSERT INTO t_d35c5cae VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_d35c5cae ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_d35c5cae FORCE ROW LEVEL SECURITY;
CREATE POLICY p_d35c5cae ON t_d35c5cae FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_d35c5cae TO matrix_grantee;
\c - postgres
SELECT id FROM t_d35c5cae;
-- rls_select/forced/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_e82ff0e4 (id integer PRIMARY KEY, c integer);
INSERT INTO t_e82ff0e4 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_e82ff0e4 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_e82ff0e4 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_e82ff0e4 ON t_e82ff0e4 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_e82ff0e4 TO matrix_grantee;
SELECT id FROM t_e82ff0e4;
-- rls_select/forced/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_9f096c96 (id integer PRIMARY KEY, c integer);
INSERT INTO t_9f096c96 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_9f096c96 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_9f096c96 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_9f096c96 ON t_9f096c96 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_9f096c96 TO matrix_grantee;
\c - matrix_grantee
SELECT id FROM t_9f096c96;
-- rls_select/forced_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_eba82636 (id integer PRIMARY KEY, c integer);
INSERT INTO t_eba82636 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_eba82636 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_eba82636 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_eba82636 ON t_eba82636 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_eba82636 TO matrix_grantee;
\c - postgres
SET row_security = off;
SELECT id FROM t_eba82636;
-- rls_select/forced_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_6cfc4cef (id integer PRIMARY KEY, c integer);
INSERT INTO t_6cfc4cef VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_6cfc4cef ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_6cfc4cef FORCE ROW LEVEL SECURITY;
CREATE POLICY p_6cfc4cef ON t_6cfc4cef FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_6cfc4cef TO matrix_grantee;
SET row_security = off;
SELECT id FROM t_6cfc4cef;
-- rls_select/forced_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_eb8a1647 (id integer PRIMARY KEY, c integer);
INSERT INTO t_eb8a1647 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_eb8a1647 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_eb8a1647 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_eb8a1647 ON t_eb8a1647 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_eb8a1647 TO matrix_grantee;
\c - matrix_grantee
SET row_security = off;
SELECT id FROM t_eb8a1647;
-- rls_select/nogrant/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_8b972c94 (id integer PRIMARY KEY, c integer);
INSERT INTO t_8b972c94 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_8b972c94 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_8b972c94 ON t_8b972c94 FOR SELECT USING (id = 0);
\c - postgres
SELECT id FROM t_8b972c94;
-- rls_select/nogrant/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_d8877c59 (id integer PRIMARY KEY, c integer);
INSERT INTO t_d8877c59 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_d8877c59 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_d8877c59 ON t_d8877c59 FOR SELECT USING (id = 0);
SELECT id FROM t_d8877c59;
-- rls_select/nogrant/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_2241e138 (id integer PRIMARY KEY, c integer);
INSERT INTO t_2241e138 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_2241e138 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_2241e138 ON t_2241e138 FOR SELECT USING (id = 0);
\c - matrix_grantee
SELECT id FROM t_2241e138;
-- rls_select/nogrant_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_a703cefe (id integer PRIMARY KEY, c integer);
INSERT INTO t_a703cefe VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_a703cefe ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_a703cefe ON t_a703cefe FOR SELECT USING (id = 0);
\c - postgres
SET row_security = off;
SELECT id FROM t_a703cefe;
-- rls_select/nogrant_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_d5d46192 (id integer PRIMARY KEY, c integer);
INSERT INTO t_d5d46192 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_d5d46192 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_d5d46192 ON t_d5d46192 FOR SELECT USING (id = 0);
SET row_security = off;
SELECT id FROM t_d5d46192;
-- rls_select/nogrant_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_893c0da8 (id integer PRIMARY KEY, c integer);
INSERT INTO t_893c0da8 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_893c0da8 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_893c0da8 ON t_893c0da8 FOR SELECT USING (id = 0);
\c - matrix_grantee
SET row_security = off;
SELECT id FROM t_893c0da8;
-- rls_insert/enabled/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_bb719295 (id integer PRIMARY KEY, c integer);
INSERT INTO t_bb719295 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_bb719295 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_bb719295 ON t_bb719295 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_bb719295 TO matrix_grantee;
\c - postgres
INSERT INTO t_bb719295 VALUES (99, 99);
SELECT id FROM t_bb719295;
-- rls_insert/enabled/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_4799a43b (id integer PRIMARY KEY, c integer);
INSERT INTO t_4799a43b VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_4799a43b ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_4799a43b ON t_4799a43b FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_4799a43b TO matrix_grantee;
INSERT INTO t_4799a43b VALUES (99, 99);
\c - postgres
SELECT id FROM t_4799a43b;
-- rls_insert/enabled/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_27b5f1ad (id integer PRIMARY KEY, c integer);
INSERT INTO t_27b5f1ad VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_27b5f1ad ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_27b5f1ad ON t_27b5f1ad FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_27b5f1ad TO matrix_grantee;
\c - matrix_grantee
INSERT INTO t_27b5f1ad VALUES (99, 99);
\c - postgres
SELECT id FROM t_27b5f1ad;
-- rls_insert/enabled_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_ef1f2932 (id integer PRIMARY KEY, c integer);
INSERT INTO t_ef1f2932 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_ef1f2932 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_ef1f2932 ON t_ef1f2932 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_ef1f2932 TO matrix_grantee;
\c - postgres
SET row_security = off;
INSERT INTO t_ef1f2932 VALUES (99, 99);
SELECT id FROM t_ef1f2932;
-- rls_insert/enabled_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_2776e59b (id integer PRIMARY KEY, c integer);
INSERT INTO t_2776e59b VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_2776e59b ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_2776e59b ON t_2776e59b FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_2776e59b TO matrix_grantee;
SET row_security = off;
INSERT INTO t_2776e59b VALUES (99, 99);
\c - postgres
SELECT id FROM t_2776e59b;
-- rls_insert/enabled_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_bc2596e4 (id integer PRIMARY KEY, c integer);
INSERT INTO t_bc2596e4 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_bc2596e4 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_bc2596e4 ON t_bc2596e4 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_bc2596e4 TO matrix_grantee;
\c - matrix_grantee
SET row_security = off;
INSERT INTO t_bc2596e4 VALUES (99, 99);
\c - postgres
SELECT id FROM t_bc2596e4;
-- rls_insert/forced/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_ee4895c6 (id integer PRIMARY KEY, c integer);
INSERT INTO t_ee4895c6 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_ee4895c6 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_ee4895c6 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_ee4895c6 ON t_ee4895c6 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_ee4895c6 TO matrix_grantee;
\c - postgres
INSERT INTO t_ee4895c6 VALUES (99, 99);
SELECT id FROM t_ee4895c6;
-- rls_insert/forced/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_990c2477 (id integer PRIMARY KEY, c integer);
INSERT INTO t_990c2477 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_990c2477 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_990c2477 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_990c2477 ON t_990c2477 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_990c2477 TO matrix_grantee;
INSERT INTO t_990c2477 VALUES (99, 99);
\c - postgres
SELECT id FROM t_990c2477;
-- rls_insert/forced/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_35423eed (id integer PRIMARY KEY, c integer);
INSERT INTO t_35423eed VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_35423eed ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_35423eed FORCE ROW LEVEL SECURITY;
CREATE POLICY p_35423eed ON t_35423eed FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_35423eed TO matrix_grantee;
\c - matrix_grantee
INSERT INTO t_35423eed VALUES (99, 99);
\c - postgres
SELECT id FROM t_35423eed;
-- rls_insert/forced_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_73a77497 (id integer PRIMARY KEY, c integer);
INSERT INTO t_73a77497 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_73a77497 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_73a77497 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_73a77497 ON t_73a77497 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_73a77497 TO matrix_grantee;
\c - postgres
SET row_security = off;
INSERT INTO t_73a77497 VALUES (99, 99);
SELECT id FROM t_73a77497;
-- rls_insert/forced_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_f31c7083 (id integer PRIMARY KEY, c integer);
INSERT INTO t_f31c7083 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_f31c7083 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_f31c7083 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_f31c7083 ON t_f31c7083 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_f31c7083 TO matrix_grantee;
SET row_security = off;
INSERT INTO t_f31c7083 VALUES (99, 99);
\c - postgres
SELECT id FROM t_f31c7083;
-- rls_insert/forced_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_9b7ea125 (id integer PRIMARY KEY, c integer);
INSERT INTO t_9b7ea125 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_9b7ea125 ENABLE ROW LEVEL SECURITY;
ALTER TABLE t_9b7ea125 FORCE ROW LEVEL SECURITY;
CREATE POLICY p_9b7ea125 ON t_9b7ea125 FOR SELECT USING (id = 0);
GRANT SELECT, INSERT ON t_9b7ea125 TO matrix_grantee;
\c - matrix_grantee
SET row_security = off;
INSERT INTO t_9b7ea125 VALUES (99, 99);
\c - postgres
SELECT id FROM t_9b7ea125;
-- rls_insert/nogrant/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_78894d89 (id integer PRIMARY KEY, c integer);
INSERT INTO t_78894d89 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_78894d89 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_78894d89 ON t_78894d89 FOR SELECT USING (id = 0);
\c - postgres
INSERT INTO t_78894d89 VALUES (99, 99);
SELECT id FROM t_78894d89;
-- rls_insert/nogrant/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_c10669bf (id integer PRIMARY KEY, c integer);
INSERT INTO t_c10669bf VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_c10669bf ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_c10669bf ON t_c10669bf FOR SELECT USING (id = 0);
INSERT INTO t_c10669bf VALUES (99, 99);
\c - postgres
SELECT id FROM t_c10669bf;
-- rls_insert/nogrant/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_375e6e6c (id integer PRIMARY KEY, c integer);
INSERT INTO t_375e6e6c VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_375e6e6c ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_375e6e6c ON t_375e6e6c FOR SELECT USING (id = 0);
\c - matrix_grantee
INSERT INTO t_375e6e6c VALUES (99, 99);
\c - postgres
SELECT id FROM t_375e6e6c;
-- rls_insert/nogrant_rowsec_off/superuser/plain/bare
\c - matrix_owner
CREATE TABLE t_b70d6406 (id integer PRIMARY KEY, c integer);
INSERT INTO t_b70d6406 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_b70d6406 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_b70d6406 ON t_b70d6406 FOR SELECT USING (id = 0);
\c - postgres
SET row_security = off;
INSERT INTO t_b70d6406 VALUES (99, 99);
SELECT id FROM t_b70d6406;
-- rls_insert/nogrant_rowsec_off/owner/plain/bare
\c - matrix_owner
CREATE TABLE t_75b1a447 (id integer PRIMARY KEY, c integer);
INSERT INTO t_75b1a447 VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_75b1a447 ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_75b1a447 ON t_75b1a447 FOR SELECT USING (id = 0);
SET row_security = off;
INSERT INTO t_75b1a447 VALUES (99, 99);
\c - postgres
SELECT id FROM t_75b1a447;
-- rls_insert/nogrant_rowsec_off/grantee/plain/bare
\c - matrix_owner
CREATE TABLE t_197643cd (id integer PRIMARY KEY, c integer);
INSERT INTO t_197643cd VALUES (1, 1), (2, 2), (3, 3);
ALTER TABLE t_197643cd ENABLE ROW LEVEL SECURITY;
CREATE POLICY p_197643cd ON t_197643cd FOR SELECT USING (id = 0);
\c - matrix_grantee
SET row_security = off;
INSERT INTO t_197643cd VALUES (99, 99);
\c - postgres
SELECT id FROM t_197643cd;
