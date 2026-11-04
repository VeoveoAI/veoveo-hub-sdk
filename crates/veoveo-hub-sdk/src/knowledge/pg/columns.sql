-- The columns of one table among those a knowledge source names: each column's type,
-- the type's schema, its modifier and whether it is generated.
-- $1 schema, $2 table, $3 column names.
SELECT
    a.attname::text AS name,
    t.typname::text AS type,
    tn.nspname::text AS type_schema,
    a.atttypmod AS modifier,
    a.attgenerated::text AS generated
FROM pg_catalog.pg_attribute AS a
JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_type AS t ON t.oid = a.atttypid
JOIN pg_catalog.pg_namespace AS tn ON tn.oid = t.typnamespace
WHERE n.nspname = $1
    AND c.relname = $2
    AND c.relkind IN ('r', 'p')
    AND a.attnum > 0
    AND NOT a.attisdropped
    AND a.attname = ANY ($3)
