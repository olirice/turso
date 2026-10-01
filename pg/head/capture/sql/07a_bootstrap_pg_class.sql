SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT c.*
  FROM pg_class c
  WHERE c.relnamespace = 11
    AND (
      c.relkind = 'r'
      OR c.oid IN (SELECT indexrelid FROM pg_index WHERE indisunique)
      OR c.relname IN ('pg_roles', 'pg_settings', 'pg_seclabels')
    )
) t;
