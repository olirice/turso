SELECT json_agg(row_to_json(c) ORDER BY c.source, c.target) FROM (
  SELECT
    pc.castsource AS source,
    src.typname AS source_name,
    pc.casttarget AS target,
    tgt.typname AS target_name
  FROM pg_cast pc
  JOIN pg_type src ON src.oid = pc.castsource
  JOIN pg_type tgt ON tgt.oid = pc.casttarget
  WHERE pc.castcontext = 'i'
) c;
