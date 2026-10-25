# Hub Client Design

`veoveo-hub-sdk-cli` is a command-line MCP client for one installation's Hub,
built on the SDK's `HubClient`. It acts as one of the installation's OAuth clients: it lists the tools, resources and
prompts that caller sees, reads a resource, calls a tool and waits for a Task-backed
tool to end. Its `ask` command lets a chat model work on a request with the caller's
tools, and every call the model makes reaches the Hub as an ordinary call from that
caller. A person signs in with `login` through their browser, and the other commands then
act as that person with the stored grant, which the CLI renews by itself. The use-case
tests drive the same modules against a running installation.

## Standards And Protocols

| Standard or protocol | Supported profile |
|---|---|
| Model Context Protocol | version `2026-07-28` with the Discover lifecycle, through the SDK's `HubClient`; `tools/list`, `resources/list`, `resources/templates/list` and `prompts/list` read every page within the SDK catalog's bounds; `resources/read` and `tools/call`; a Task-backed call waits for the Task's status through `subscriptions/listen`; the client answers no `input_required`, elicitation or sampling request |
| MCP Streamable HTTP | the rmcp 3.5.0 client transport, from the `VeoveoAI/rust-sdk` fork the workspace pins, with the caller's access token as a Bearer credential |
| OAuth 2.0 client credentials, RFC 6749 section 4.4 | the SDK's `ClientCredentialsBearer`: the caller's token for the profile's MCP endpoint, requested with `resource`, RFC 8707, set to that endpoint and the configured scope |
| Protected resource metadata, RFC 9728, and authorization server metadata, RFC 8414 | the bearer finds the authorization server in the profile endpoint's metadata and the token endpoint in that server's metadata; `login` also reads the authorization and revocation endpoints and requires `S256` in `code_challenge_methods_supported` |
| OAuth 2.0 authorization code grant, RFC 6749 section 4.1, with PKCE, RFC 7636 | `login` as a public client: an `S256` challenge of a 32-byte random verifier, a 32-byte random `state`, `scope` from `--scope` and `resource`, RFC 8707, set to the profile's protected resource; the code exchange repeats `redirect_uri` and `resource` and sends no scope, which Hub refuses there |
| OAuth 2.0 for Native Apps, RFC 8252 | the loopback redirect of section 7.3: plain HTTP to an IP literal, section 8.3, on the registered port, since Hub matches a redirect URI exactly; the system browser opens the authorization endpoint |
| Refresh tokens, RFC 6749 section 6 | the stored grant renews with `grant_type=refresh_token`, `client_id` and `resource` and no scope, which Hub refuses on refresh; a rotated refresh token replaces the spent one, and a response without one leaves the current one in force |
| Token revocation, RFC 7009 | `logout` revokes the refresh token with `token_type_hint=refresh_token` when the metadata names a revocation endpoint |
| JWT client authentication, RFC 7523 | an RS256 `client_assertion` signed with the caller's private key, whose key ID is the first key of the caller's JWK Set |
| RFC 6570 URI Templates | the use-case tests address an event by expanding the template Time advertises with core's `ResourceTemplateUri` |
| Chat Completions API, OpenAI-compatible | an adapter the CLI owns, not a Veoveo contract: non-streaming `POST {base}/chat/completions` with each MCP tool as a `function` whose `parameters` is its input schema as the Hub publishes it, `tool_choice` `auto` and `max_tokens`; qualified with the Hugging Face Inference Providers router |
| JSON Schema 2020-12 | tool input schemas pass to the model unchanged, in the profile [the server contract](../veoveo-hub-sdk/DESIGN.md#schemas-and-types) publishes, where every typed argument declares one `type` |
| dotenv files | `--env-file` reads one `NAME=value` line for the model token; quoting, `export` and interpolation are outside the profile |

## Commands

| Command | Does |
|---|---|
| `login` | Signs the person in through their browser and stores the grant |
| `logout` | Revokes the stored grant's refresh token at the Hub and deletes the grant |
| `tools` | Lists the caller's tools with their annotations and input schemas |
| `resources` | Lists the caller's resources and resource templates |
| `prompts` | Lists the caller's prompts |
| `read <uri>` | Reads one resource and prints the result |
| `call <tool> [arguments]` | Calls one tool with a JSON object of arguments and prints its result; a Task-backed tool runs to its end within `--timeout-seconds`; an error result exits with a failure status |
| `ask <request>` | Lets a chat model work on the request with the caller's tools and prints each step and the answer |

Global flags name the Hub and the caller. `--hub` is the Hub's public origin
(default `http://vv-hub.internal:8080`), which tokens and assertions name. `--keys`
is the directory holding `<caller>.pem` and `<caller>.jwks.json`, and `--caller` the
OAuth client to act as (default `acceptance-member`). `--connect` sends requests for the
origin's host to another socket address while each request keeps the origin's name, so
a host reaches a local installation's published edge:

```sh
veoveo-hub-sdk-cli --connect 127.0.0.1:8080 --keys state/callers tools
veoveo-hub-sdk-cli --connect 127.0.0.1:8080 --keys state/callers --caller acceptance-admin \
  ask --env-file .env "Generate an image of a red circle with the fake text-to-image model."
```

`--bearer-env HUB_TOKEN` (or `VEOVEO_CLI_BEARER_ENV=HUB_TOKEN`) uses the
existing bearer in that variable through `StaticBearer`. It accepts native opaque
credential references and requires no caller key, caller ID or scope. It conflicts
with `--keys`; issuance and renewal belong to the caller's trusted runtime.

With neither `--keys` nor `--bearer-env`, a command acts as the person whose grant
`login` stored for the `--hub` origin and `--profile`. `--grants` (or
`VEOVEO_CLI_GRANT_DIRECTORY`) names the directory of grants, by default
`$XDG_CONFIG_HOME/veoveo-hub-sdk-cli/grants`, or `~/.config/veoveo-hub-sdk-cli/grants`
when `XDG_CONFIG_HOME` names no absolute directory. `login` and `logout` act on that
directory whatever `--keys` or `--bearer-env` say:

```sh
veoveo-hub-sdk-cli --hub https://veoveo.example.com login
veoveo-hub-sdk-cli --hub https://veoveo.example.com tools
veoveo-hub-sdk-cli --hub https://veoveo.example.com logout
```

`--json` prints JSON: the listing, the call result or the whole `ask` transcript. Each
flag also reads a `VEOVEO_CLI_*` environment variable.

## Client And Token

With `--keys`, the CLI reads the caller's key with the SDK's `caller_signer` and builds
one `HubClient` with a `ClientCredentialsBearer`; with the stored grant, the bearer is the
grant's own (see [Sign-In And Stored Grant](#sign-in-and-stored-grant)). Every request carries the bearer's
current token, and the bearer requests a new one once a third of the token's lifetime
remains or the Hub stops admitting it. Calls by tool name use a connection per
server the name namespaces; listings and reads use one connection for the caller's
profile-wide requests. A listing that misses a server's surface still prints what the
Hub returned and names each missing surface on standard error. Token and assertion
values never reach output or `Debug` formatting.

## Sign-In And Stored Grant

`login` signs in as a public client that the installation registers with the
`authorization_code_pkce` and `refresh_token` grants, auth method `none` and a loopback
redirect. `--client` (default `veoveo-hub-sdk-cli`) and `--redirect-uri` (default
`http://127.0.0.1:3211/callback`) name that registration. The defaults are the CLI's
own registration in the installation template, for the person's `local` profile; an
installation that registers another client names it with these flags. Hub matches a
redirect URI exactly, port included, so the port is fixed, and 3211 stays clear of
OpenCode's 3210. The CLI listens on the redirect's
address before it opens the authorization endpoint with the platform's opener (`open`
on macOS, `xdg-open` elsewhere). `--no-browser` prints the address instead, which a
person opens in a browser that reaches the Hub and the listener. The listener answers
every request: one for another path or with another `state` gets an error page and is
ignored, so a stale tab or a stray request can neither end nor take over the sign-in.
A response with this sign-in's `state` ends the wait, with its code or its OAuth
error; `--timeout-seconds` (default 300) bounds it.

The grant is one JSON file per Hub origin and profile,
`<grants>/<scheme>_<host>_<port>_<profile>.json`. It holds the tokens, their expiry
times, the client, the protected resource, and the token and revocation endpoints, so
renewal needs no discovery. The directory is created with mode 0700 and the file with
mode 0600; the CLI refuses a grant file that other users can read, and one that names
another Hub or profile. Every write goes to a new file in the same directory that is
synced and then renamed over the grant, so a reader sees the whole previous grant or
the whole new one.

The grant's bearer serves the access token until a third of its lifetime remains or the
Hub stops admitting it, then renews it. A renewal holds the grant's lock, an exclusive
file lock on `<grants>/.<scheme>_<host>_<port>_<profile>.lock`, so one process renews at a time; a
process waits up to 60 seconds for another to finish. Under the lock it reads the grant
again: when another process has renewed it, it takes that access token and never sends
the refresh token it held, which that process spent. Hub revokes the whole family when
a spent refresh token returns after its successor's delivery window. Before it sends a
refresh token, the CLI stores the grant without it, recording when the renewal was
sent; the answer's grant then replaces that record. So a refresh token reaches the
token endpoint at most once. A request that never connected restores the unsent token.
A refusal, a lost answer or a process that ends mid-renewal leave a grant with no
refresh token, which serves until its access token expires and then asks the person to
sign in again. Hub would answer a repeat of a refresh token within its successor's
delivery window with that same successor, but the CLI does not resend: the window is
Hub's setting, not part of its metadata, and a repeat after it revokes the family. A
lost answer therefore costs one new `login`.

Every error that a new sign-in resolves ends with ``run `veoveo-hub-sdk-cli login` ``:
no grant, an expired or refused renewal, or a renewal whose answer did not arrive.
`logout` revokes the refresh token at the Hub when the grant names a revocation
endpoint and deletes the grant whether or not the Hub confirms; it says when the Hub did
not. A later `login` replaces the grant without revoking its predecessor, whose
refresh token stays valid at the Hub until it expires; `logout` first revokes it.

Tokens, codes and verifiers travel only in request bodies, never on the command line,
in the authorization address or in the CLI's output, and their `Debug` forms are
redacted. The OAuth requests follow no redirect, so a code, verifier or refresh token
reaches only the endpoint the metadata names. Every request the CLI sends, to the Hub
and to the model endpoint, carries the User-Agent `veoveo-hub-sdk-cli/<version>`.

`tests/sign_in.rs` runs the sign-in against a local fake authorization server that
keeps Hub's rules: exact redirect matching, S256 verification, single-use codes, no
scope on the code exchange or refresh, rotation, and revocation of the family when a
spent refresh token returns. It checks the stored grant's mode and atomic replacement,
a stray callback, a refused sign-in, renewal, two processes renewing at once, a
process holding a spent refresh token, a lost answer, a refused renewal, an unreachable
token endpoint, `logout`'s revocation and the binary's own `login --no-browser` and
`logout`.

## Ask

`ask` sends the model a system message that names the installation's tools as the way
to act and states the current UTC time, then the request. Each turn is one completion.
The model's tool calls in a turn run in order, each through the Hub as the caller,
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

## Distribution

The `cli` target of the repository's `Dockerfile` builds an image that holds only
`/usr/local/bin/veoveo-hub-sdk-cli`, compiled in the `dev` stage's Debian trixie. An
installation does not copy the binary into another image: it mounts this image
read-only into the workloads that use it, as a Kubernetes image volume, where the
binary runs on the C library of the image it is mounted into, a glibc at least as new
as Debian trixie's. A developer installs it with Cargo from a revision of
this repository.

## Use-Case Tests

`tests/use_cases.rs` gives a chat model a request a person would make and checks what
the installation did, never the model's wording:

| Test | Request | Outcome checked |
|---|---|---|
| `a_member_schedules_an_event_that_time_then_serves` | the member creates a named event for tomorrow and asks for the time in Tokyo | the model answered, a successful `time__create_event` holds the name, and the address that Time's advertised `time://events/{event_id}` template gives the event serves the same `eventId` and name |
| `an_admin_generates_an_image_that_artifact_stores` | the admin generates an image with the fake text-to-image model | a successful `media__run` names an artifact, and Artifact holds it as `image/png` |
| `a_member_cannot_run_a_billed_generation` | the member asks for the same image | no `media__run` call succeeded |

The tests are ignored by default because they need a running installation with its
fake media provider, the callers' keys and a model endpoint. Each takes about 30
seconds with the default model:

```sh
VEOVEO_CLI_KEY_DIRECTORY=<keys> VEOVEO_CLI_CONNECT=127.0.0.1:8080 HF_TOKEN=<token> \
  cargo nextest run -p veoveo-hub-sdk-cli --test use_cases --run-ignored only
```

`VEOVEO_CLI_HUB`, `VEOVEO_CLI_MODEL` and `VEOVEO_CLI_MODEL_URL` select another
installation, model or endpoint.
