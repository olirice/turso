SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT * FROM pg_authid WHERE oid IN (10, 6171) OR oid < 16384
) t;
