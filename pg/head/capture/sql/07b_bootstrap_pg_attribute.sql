SELECT json_agg(row_to_json(t) ORDER BY t.attrelid, t.attnum) FROM (
  SELECT a.*
  FROM pg_attribute a
  JOIN pg_class c ON c.oid = a.attrelid
  WHERE c.relnamespace = 11
    AND (
      c.relkind = 'r'
      OR c.oid IN (SELECT indexrelid FROM pg_index WHERE indisunique)
      OR c.relname IN ('pg_roles', 'pg_settings', 'pg_seclabels')
    )
    AND a.attnum <> 0
    AND NOT a.attisdropped
) t;
