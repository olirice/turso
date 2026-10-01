SELECT json_agg(row_to_json(t) ORDER BY t.classoid, t.objoid, t.objsubid) FROM (
  SELECT * FROM pg_init_privs
  WHERE (classoid = 'pg_namespace'::regclass AND objoid IN (11, 2200))
     OR (classoid = 'pg_class'::regclass AND objoid IN (
           SELECT c.oid
           FROM pg_class c
           WHERE c.relnamespace = 11
             AND (
               c.relkind = 'r'
               OR c.oid IN (SELECT indexrelid FROM pg_index WHERE indisunique)
               OR c.relname IN ('pg_roles', 'pg_settings', 'pg_seclabels')
             )
         ))
     OR (classoid = 'pg_proc'::regclass AND objoid IN (
           SELECT oid FROM pg_proc
           WHERE oid IN (
             'pg_catalog.set_config(text,text,boolean)'::regprocedure,
             'pg_catalog.current_setting(text)'::regprocedure,
             'pg_catalog.current_setting(text,boolean)'::regprocedure,
             'pg_catalog.current_schemas(boolean)'::regprocedure,
             'pg_catalog.current_database()'::regprocedure,
             'pg_catalog.pg_is_in_recovery()'::regprocedure,
             'pg_catalog.unnest(anyarray)'::regprocedure,
             'pg_catalog.pg_options_to_table(text[])'::regprocedure,
             'pg_catalog.heap_tableam_handler(internal)'::regprocedure,
             'pg_catalog.bthandler(internal)'::regprocedure,
             'pg_catalog.hashhandler(internal)'::regprocedure,
             'pg_catalog.gisthandler(internal)'::regprocedure,
             'pg_catalog.ginhandler(internal)'::regprocedure,
             'pg_catalog.brinhandler(internal)'::regprocedure,
             'pg_catalog.spghandler(internal)'::regprocedure,
             'pg_catalog.array_upper(anyarray,int4)'::regprocedure,
             'pg_catalog.quote_ident(text)'::regprocedure,
             'pg_catalog.quote_literal(text)'::regprocedure,
             'pg_catalog.format_type(oid,int4)'::regprocedure,
             'pg_catalog.acldefault("char",oid)'::regprocedure,
             'pg_catalog.pg_get_triggerdef(oid,boolean)'::regprocedure,
             'pg_catalog.pg_get_constraintdef(oid)'::regprocedure,
             'pg_catalog.pg_get_constraintdef(oid,boolean)'::regprocedure,
             'pg_catalog.pg_get_indexdef(oid)'::regprocedure,
             'pg_catalog.pg_get_indexdef(oid,int4,boolean)'::regprocedure,
             'pg_catalog.pg_get_expr(pg_node_tree,oid)'::regprocedure,
             'pg_catalog.array_remove(anycompatiblearray,anycompatible)'::regprocedure,
             'pg_catalog.array_to_string(anyarray,text)'::regprocedure,
             'pg_catalog.generate_series(int4,int4)'::regprocedure,
             'pg_catalog.array_agg(anynonarray)'::regprocedure,
             'pg_catalog.count()'::regprocedure
           )
           OR (proname = 'count' AND pronamespace = 11 AND pronargs = 1)
         ))
) t;
