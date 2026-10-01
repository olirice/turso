SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT
    c.oid,
    c.relname,
    c.relkind,
    c.relnatts,
    c.relhasindex,
    c.relisshared,
    c.relpersistence,
    c.relrowsecurity,
    c.relacl::text AS relacl,
    c.reltype,
    c.relam
  FROM pg_class c
  WHERE c.relnamespace = 11
    AND (c.relkind IN ('r', 'i') OR c.relname IN ('pg_roles', 'pg_settings', 'pg_seclabels'))
) t;
