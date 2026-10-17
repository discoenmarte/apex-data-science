-- Total size of the public schema
SELECT
  pg_size_pretty(sum(pg_total_relation_size(c.oid))) AS public_schema_size,
  sum(pg_total_relation_size(c.oid)) AS public_schema_size_bytes
FROM
  pg_class c
  JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE
  n.nspname = 'public'
  AND c.relkind = 'r';
