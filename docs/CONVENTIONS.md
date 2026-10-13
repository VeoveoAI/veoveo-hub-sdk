# MCP Naming Conventions

These conventions extend [veoveo-core's naming and format conventions](https://github.com/VeoveoAI/veoveo-core/blob/f685fbfd3d28a477d35f77cee3feacc8787772ac/docs/CONVENTIONS.md)
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

A server's URI scheme equals its slug. The Gateway projects a tool as `<slug>__<tool>`
when it lists tools from several servers. Resource templates use RFC 6570 simple
expansion, and lists page through MCP cursors.

## Errors

JSON-RPC errors use MCP's codes, plus Veoveo codes from `-31999` to `-31000`, outside the
range JSON-RPC 2.0 reserves. `veoveo_sdk_rust::jsonrpc_codes` lists them.

## HTTP Paths

A hosted server serves MCP at `/<slug>/mcp`. Plain `http` is allowed only for local
hosts, the same exception MCP's authorization specification makes for localhost.
