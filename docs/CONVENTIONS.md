# MCP Naming Conventions

These conventions extend [veoveo-core's naming and format conventions](https://github.com/VeoveoAI/veoveo-core/blob/0e4f26ab0f15b27509bdf6a3796df2f8041cbfb0/docs/CONVENTIONS.md)
for MCP. Where a standard governs a name, the standard wins.

## Standards And Protocols

| Standard or protocol | Governs |
|---|---|
| [MCP](https://modelcontextprotocol.io/specification/) `2026-07-28` and its extension rules | MCP messages, tool names, `_meta` keys and `capabilities.extensions` identifiers |
| [JSON-RPC 2.0](https://www.jsonrpc.org/specification) | Request and error envelopes |

## JSON Casing

MCP messages use `camelCase`, as Veoveo HTTP API bodies, configuration files and
Veoveo formats do.

## Extension Names

MCP extension identifiers and `_meta` keys take the form `ai.veoveo/<kebab-name>`, such
as `ai.veoveo/registration`. A name is one concept, such as `task` or `hub`; its
features are typed members of its payload, never names of their own. A contract a
server declares compliance with is identified by `https://veoveo.ai/contracts/<kebab-name>`,
such as `https://veoveo.ai/contracts/hosted-mcp`.

## Skill Addresses

A server's skill files are `skill://{slug}/{name}/{path}`: the scheme `skill`, the
server's slug, the skill's name, which is its directory's, and the file's path in the
skill, `/`-separated; the skill itself is the address of its `SKILL.md`. `skill://veoveo/`
is the Hub's own skills'. Skill text names a tool as its callers see it through the Hub,
`{slug}__{tool}`, and a linked server's tool with its link, such as
`n8n__{link}__run`.

## Domain Names

A concept has one noun across every surface, and each surface spells it in its own form:

| Surface | Form | Time's event |
|---|---|---|
| MCP tool | `<verb>_<noun>` in `snake_case` | `create_event`, `cancel_event` |
| Resource URI | `<slug>://<plural>/{id}` | `time://events/{event_id}` |
| Resource type | `<slug>.<singular>` | `time.event` |
| Permission | `<slug>.<plural>.<verb>` | `time.events.write` |
| Knowledge collection | `<slug>.<plural>` | `time.events` |

A server's URI scheme equals its slug. The Hub projects a tool as `<slug>__<tool>`
when it lists tools from several servers, and a tool of a server served by the runners
of linked repositories as `<slug>__<link>__<tool>`, where the link is a lowercase DNS
label; a slug holds no `__`, so a name splits at its first `__`
(`HubToolName::projection`). Resource templates use RFC 6570 simple
expansion, and lists page through MCP cursors.

## Knowledge

A server whose records people search names each searchable collection
`<slug>.<plural>`, a `KnowledgeCollection`: its slug and the plural noun of its records,
such as `artifact.artifacts`. The plural starts with a lowercase ASCII letter and holds
lowercase ASCII letters, digits, `-` or `_`. The records' template carries
`_meta["ai.veoveo/knowledge"]`, `{"collection": "<slug>.<plural>"}`, and the server
serves one search tool, `search_knowledge`, which callers see as
`<slug>__search_knowledge`. Skill text names the tool so, and the collections by their
names.

A collection the SDK's PostgreSQL search ranks keeps, in its table in the server's
schema, a key column of type `uuid` or `text` and three columns of fixed names:
`knowledge_text`, a stored generated `tsvector` with a GIN index; `knowledge_vector`,
`_extensions.vector(768)` with an HNSW index over `_extensions.vector_cosine_ops`; and
`knowledge_model`, `text`, the checkpoint of the model that made the vector. Its read rule
is a function in the same schema, `(r <table>, caller jsonb) RETURNS boolean`, `STABLE`,
whose `caller` is the `KnowledgeCaller` JSON: `tenantId`, `tenantKey`, `principalId`,
`principalKey`, `groups` of `{"group", "role"}`, `dataLabels` and `asOf`. pgvector is
in the installation's `_extensions` schema, the one schema outside its own a server's SQL
names.

## Errors

JSON-RPC errors use MCP's codes, plus Veoveo codes from `-31999` to `-31000`, outside the
range JSON-RPC 2.0 reserves. `veoveo_hub_sdk::jsonrpc_codes` lists them.

## HTTP Paths

A hosted server serves MCP at `/<slug>/mcp`. Plain `http` is allowed only for local
hosts, the same exception MCP's authorization specification makes for localhost.

## Execution Routing And Deployment Identity

The Hub resolves one immutable composition, from logical slug to deployment, for each
request; the resource URL and logical tool names do not change. A client selects no
routing: there are no named environments and no selector header.

`ExecutionContext` retains the `CompositionRevision` in the signed Hub request context.
The per-hop target `deployment` is separate and must match the recipient's admitted
token trust. Downstream calls and Tasks retain the captured composition while their
target can change. Data authorization remains separate.

`https://veoveo.ai/formats/resolved-composition` identifies canonical compact UTF-8
JSON with fixed field order `format`, `formatVersion` (2), `components`. Version 1
also named an environment and is no longer read.
Components are ordered by logical slug and explicitly select a deployment, the
`runners` of a linked branch, one deployment for each link name in lexical order, or
`unavailable`; omission does not instruct a fallback. SHA-256 covers the format URI,
one NUL byte, then those exact JSON bytes. Labels and transient health are excluded.
Import recomputes the revision and rejects noncanonical or unsupported records.
