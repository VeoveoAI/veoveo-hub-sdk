-- The single-column indexes of one table: each index's access method, its column and
-- the operator class it uses, with that class's schema.
-- $1 schema, $2 table.
SELECT
    am.amname::text AS method,
    a.attname::text AS column_name,
    oc.opcname::text AS operator_class,
    ocn.nspname::text AS operator_class_schema
FROM pg_catalog.pg_index AS i
JOIN pg_catalog.pg_class AS c ON c.oid = i.indrelid
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class AS ic ON ic.oid = i.indexrelid
JOIN pg_catalog.pg_am AS am ON am.oid = ic.relam
JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attnum = i.indkey[0]
JOIN pg_catalog.pg_opclass AS oc ON oc.oid = i.indclass[0]
JOIN pg_catalog.pg_namespace AS ocn ON ocn.oid = oc.opcnamespace
WHERE n.nspname = $1
    AND c.relname = $2
    AND i.indnkeyatts = 1
    AND i.indisvalid
