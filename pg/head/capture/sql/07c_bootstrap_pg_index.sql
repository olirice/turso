SELECT json_agg(row_to_json(t) ORDER BY t.indexrelid) FROM (
  SELECT i.*
  FROM pg_index i
  JOIN pg_class c ON c.oid = i.indrelid
  WHERE c.relnamespace = 11
    AND i.indisunique
) t;
