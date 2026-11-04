-- The volatility of the read rule `$1.$3($1.$2, jsonb) RETURNS boolean`, when it exists.
-- $1 schema, $2 table, $3 function.
SELECT p.provolatile::text AS volatility
FROM pg_catalog.pg_proc AS p
JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace
WHERE n.nspname = $1
    AND p.proname = $3
    AND p.prokind = 'f'
    AND p.pronargs = 2
    AND p.proargtypes[0] = (
        SELECT c.reltype
        FROM pg_catalog.pg_class AS c
        JOIN pg_catalog.pg_namespace AS cn ON cn.oid = c.relnamespace
        WHERE cn.nspname = $1 AND c.relname = $2
    )
    AND p.proargtypes[1] = 'pg_catalog.jsonb'::pg_catalog.regtype
    AND p.prorettype = 'pg_catalog.bool'::pg_catalog.regtype
    AND NOT p.proretset
