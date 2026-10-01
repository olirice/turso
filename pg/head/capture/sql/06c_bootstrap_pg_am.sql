SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT *, amhandler::oid AS amhandler FROM pg_am
) t;
