-- Get public schema blueprint
WITH tables AS (
    SELECT
        n.nspname AS schema_name,
        c.relname AS table_name,
        obj_description(c.oid) AS table_comment
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relkind = 'r'  -- ordinary tables
      AND n.nspname = 'public'
)
SELECT json_agg(
    json_build_object(
        'schema', t.schema_name,
        'table', t.table_name,
        'comment', t.table_comment,
        'columns', (
            SELECT json_agg(
                json_build_object(
                    'name', a.attname,
                    'data_type', pg_catalog.format_type(a.atttypid, a.atttypmod),
                    'is_nullable', NOT a.attnotnull,
                    'default_value', pg_get_expr(ad.adbin, ad.adrelid),
                    'comment', col_description(a.attrelid, a.attnum)
                )
                ORDER BY a.attnum
            )
            FROM pg_attribute a
            LEFT JOIN pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum
            WHERE a.attrelid = (quote_ident(t.schema_name) || '.' || quote_ident(t.table_name))::regclass
              AND a.attnum > 0
              AND NOT a.attisdropped
        ),
        'constraints', (
            SELECT json_agg(
                json_build_object(
                    'constraint_name', con.conname,
                    'constraint_type', con.contype,
                    'definition', pg_get_constraintdef(con.oid)
                )
            )
            FROM pg_constraint con
            WHERE con.conrelid = (quote_ident(t.schema_name) || '.' || quote_ident(t.table_name))::regclass
        )
    )
)
FROM tables t;

