SELECT json_agg(row_to_json(t)) FROM (
  SELECT * FROM pg_description
  WHERE objoid IN (11, 2200) AND classoid = 'pg_namespace'::regclass
) t;
