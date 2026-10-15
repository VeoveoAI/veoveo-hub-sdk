# Gateway Client Design

`veoveo-sdk-rust-cli` is a command-line MCP client for one installation's Gateway. It
acts as one of the installation's OAuth clients: it lists the tools, resources and
prompts that caller sees, reads a resource, calls a tool and waits for a Task-backed
tool to end. Its `ask` command lets a chat model work on a request with the caller's
tools, and every call the model makes reaches the Gateway as an ordinary call from that
caller. The use-case tests drive the same modules against a running installation.

## Standards And Protocols

| Standard or protocol | Supported profile |
|---|---|
| Model Context Protocol | version `2026-07-28` with the Discover lifecycle; `tools/list`, `resources/list`, `resources/templates/list` and `prompts/list` read every page; `resources/read`, `prompts/get` and `tools/call`; a Task-backed call polls `tasks/get` at the Task's poll interval, held between 50 ms and 5 s; the client answers no `input_required`, elicitation or sampling request |
| MCP Streamable HTTP | the rmcp 3.5.0 client transport, from the `VeoveoAI/rust-sdk` fork the workspace pins, with the caller's access token as a Bearer credential |
| OAuth 2.0 client credentials, RFC 6749 section 4.4 | the caller's token for the profile's MCP endpoint, requested with `resource`, RFC 8707, set to that endpoint and the configured scope |
| JWT client authentication, RFC 7523 | an RS256 `client_assertion` signed with the caller's private key, whose key ID is the first key of the caller's JWK Set |
| Chat Completions API, OpenAI-compatible | an adapter the CLI owns, not a Veoveo contract: non-streaming `POST {base}/chat/completions` with each MCP tool as a `function` whose `parameters` is its input schema as the Gateway publishes it, `tool_choice` `auto` and `max_tokens`; qualified with the Hugging Face Inference Providers router |
| JSON Schema 2020-12 | tool input schemas pass to the model unchanged, in the profile [the server contract](../veoveo-sdk-rust/DESIGN.md#schemas-and-types) publishes, where every typed argument declares one `type` |
| dotenv files | `--env-file` reads one `NAME=value` line for the model token; quoting, `export` and interpolation are outside the profile |

## Commands

| Command | Does |
|---|---|
| `tools` | Lists the caller's tools with their annotations and input schemas |
| `resources` | Lists the caller's resources and resource templates |
| `prompts` | Lists the caller's prompts |
| `read <uri>` | Reads one resource and prints the result |
| `call <tool> [arguments]` | Calls one tool with a JSON object of arguments and prints its result; a Task-backed tool runs to its end within `--timeout-seconds`; an error result exits with a failure status |
| `ask <request>` | Lets a chat model work on the request with the caller's tools and prints each step and the answer |

Global flags name the Gateway and the caller. `--gateway` is the Gateway's public origin
(default `http://vv-gateway.internal:8080`), which tokens and assertions name. `--keys`
is the directory holding `<caller>.pem` and `<caller>.jwks.json`, and `--caller` the
OAuth client to act as (default `acceptance-member`). `--connect` sends requests for the
origin's host to another socket address while each request keeps the origin's name, so
a host reaches a local installation's published edge:

```sh
veoveo-sdk-rust-cli --connect 127.0.0.1:8080 --keys state/callers tools
veoveo-sdk-rust-cli --connect 127.0.0.1:8080 --keys state/callers --caller acceptance-admin \
  ask --env-file .env "Generate an image of a red circle with the fake text-to-image model."
```

`--json` prints JSON: the listing, the call result or the whole `ask` transcript. Each
flag also reads a `VEOVEO_CLI_*` environment variable.

## Session And Token

A session requests a token, opens an MCP session with it and keeps both. Before each
tool call the `ask` loop makes, the session checks the token. Once a third of its
lifetime remains, the session requests a new token and opens a new MCP session with it,
then closes the old one. Token and assertion values never reach output or `Debug`
formatting.

## Ask

`ask` sends the model a system message that names the installation's tools as the way
to act and states the current UTC time, then the request. Each turn is one completion.
The model's tool calls in a turn run in order, each through the Gateway as the caller,
and their results return to the model as JSON holding `isError`, the text and resource
links of the content and the `structuredContent`. The loop stops when the model answers
without a tool call, when it runs out of output tokens, or after `--max-turns` turns
(default 16). An argument string that is not a JSON object, a refusal and a failed Task
each return to the model as an error result, so the model can correct itself.

A completion may take 300 seconds. The endpoint gets three attempts when it answers 502,
503 or 504, times out or refuses the connection, with 2 and 4 seconds between them. The
model token comes from the variable `--model-token-env` names (default `HF_TOKEN`) or
from `--env-file`, and the CLI sends it only to the model endpoint. The default model is
`XiaomiMiMo/MiMo-V2.6-Pro-RL:novita` through `https://router.huggingface.co/v1`.

## Use-Case Tests

`tests/use_cases.rs` gives a chat model a request a person would make and checks what
the installation did, never the model's wording:

| Test | Request | Outcome checked |
|---|---|---|
| `a_member_schedules_an_event_that_time_then_serves` | the member creates a named event for tomorrow and asks for the time in Tokyo | the model answered, a successful `time__create_event` holds the name, and the event Time lists under that name serves the same `eventId` and name |
| `an_admin_generates_an_image_that_artifact_stores` | the admin generates an image with the fake text-to-image model | a successful `media__run` names an artifact, and Artifact holds it as `image/png` |
| `a_member_cannot_run_a_billed_generation` | the member asks for the same image | no `media__run` call succeeded |

The tests are ignored by default because they need a running installation with its
fake media provider, the callers' keys and a model endpoint. Each takes about 30
seconds with the default model:

```sh
VEOVEO_CLI_KEY_DIRECTORY=<keys> VEOVEO_CLI_CONNECT=127.0.0.1:8080 HF_TOKEN=<token> \
  cargo nextest run -p veoveo-sdk-rust-cli --test use_cases --run-ignored only
```

`VEOVEO_CLI_GATEWAY`, `VEOVEO_CLI_MODEL` and `VEOVEO_CLI_MODEL_URL` select another
installation, model or endpoint.
