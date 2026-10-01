SELECT json_agg(row_to_json(t) ORDER BY t.reloid, t.attnum) FROM (
  SELECT
    a.attrelid AS reloid,
    a.attnum,
    a.attname,
    a.atttypid,
    a.attnotnull,
    a.attlen,
    a.attalign,
    a.attstorage,
    a.attcollation,
    a.attndims,
    (a.attnum < 0) AS is_system_column
  FROM pg_attribute a
  JOIN pg_class c ON c.oid = a.attrelid
  WHERE c.relnamespace = 11
    AND c.relkind = 'r'
    AND a.attnum <> 0
    AND NOT a.attisdropped
) t;
