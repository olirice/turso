SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT
    i.indexrelid AS oid,
    i.indrelid AS table_oid,
    i.indisunique,
    i.indkey::text AS indkey
  FROM pg_index i
  JOIN pg_class tc ON tc.oid = i.indrelid
  WHERE tc.relnamespace = 11
) t;
