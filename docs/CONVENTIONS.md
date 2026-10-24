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
as `ai.veoveo/registration`. A contract a server declares compliance with is identified
by `https://veoveo.ai/contracts/<kebab-name>`, such as
`https://veoveo.ai/contracts/hosted-mcp`.

## Domain Names

A concept has one noun across every surface, and each surface spells it in its own form:

| Surface | Form | Time's event |
|---|---|---|
| MCP tool | `<verb>_<noun>` in `snake_case` | `create_event`, `cancel_event` |
| Resource URI | `<slug>://<plural>/{id}` | `time://events/{event_id}` |
| Resource type | `<slug>.<singular>` | `time.event` |
| Permission | `<slug>.<plural>.<verb>` | `time.events.write` |

A server's URI scheme equals its slug. The Hub projects a tool as `<slug>__<tool>`
when it lists tools from several servers. Resource templates use RFC 6570 simple
expansion, and lists page through MCP cursors.

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
target can change. Business WorkContext and data authorization remain separate.

`https://veoveo.ai/formats/resolved-composition` identifies canonical compact UTF-8
JSON with fixed field order `format`, `formatVersion` (2), `components`. Version 1
also named an environment and is no longer read.
Components are ordered by logical slug and explicitly select a deployment or
`unavailable`; omission does not instruct a fallback. SHA-256 covers the format URI,
one NUL byte, then those exact JSON bytes. Labels and transient health are excluded.
Import recomputes the revision and rejects noncanonical or unsupported records.
