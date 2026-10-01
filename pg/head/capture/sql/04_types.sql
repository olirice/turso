WITH used_types AS (
  SELECT DISTINCT a.atttypid AS oid
  FROM pg_attribute a
  JOIN pg_class c ON c.oid = a.attrelid
  WHERE c.relnamespace = 11
    AND a.attnum <> 0
    AND NOT a.attisdropped
),
elem_types AS (
  SELECT t.typelem AS oid
  FROM pg_type t
  JOIN used_types u ON u.oid = t.oid
  WHERE t.typelem <> 0
),
fixed_types AS (
  SELECT unnest(ARRAY[
    'bool','int2','int4','int8','text','name','"char"','oid','oid[]','int2vector','oidvector',
    'regclass','regproc','regtype','aclitem','aclitem[]','pg_node_tree','text[]','xid','float4','float8',
    'timestamptz','bytea','anyarray','void','record','cstring','int4[]'
  ]::regtype[])::oid AS oid
),
pseudo_types AS (
  SELECT oid
  FROM pg_type
  WHERE typname IN ('any', 'anynonarray', 'anycompatible', 'anycompatiblearray')
),
all_types AS (
  SELECT oid FROM used_types
  UNION
  SELECT oid FROM elem_types
  UNION
  SELECT oid FROM fixed_types
  UNION
  SELECT oid FROM pseudo_types
)
SELECT json_agg(row_to_json(t) ORDER BY t.oid) FROM (
  SELECT pt.*,
    pt.typinput::oid AS typinput,
    pt.typoutput::oid AS typoutput,
    pt.typreceive::oid AS typreceive,
    pt.typsend::oid AS typsend,
    pt.typmodin::oid AS typmodin,
    pt.typmodout::oid AS typmodout,
    pt.typanalyze::oid AS typanalyze,
    pt.typsubscript::oid AS typsubscript
  FROM pg_type pt
  JOIN all_types u ON u.oid = pt.oid
) t;
