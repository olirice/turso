SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT * FROM pg_database WHERE datname = 'postgres'
) t;
