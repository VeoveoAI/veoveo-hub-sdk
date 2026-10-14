# Server Registration Protocol

This document is the normative protocol between an MCP server and the Veoveo Gateway.
A server registers itself, declares what it owns and which permission each of its
targets requires, and keeps its registration while its process holds a live kernel
instance on the database login the registration grants. The Gateway forwards each admitted request with the caller's granted permissions, and the server
checks them again. Neither the Gateway nor the installation configuration names a
server, so every server enters the catalog through this protocol.

The `registration` module of `veoveo-sdk-rust` holds the typed declarations, the OAuth
request and response types, the software statement, the client assertion, the
granted-permissions detail and the registration client that servers run. The
`internal_auth` module issues and verifies the forwarded access token, described under
[Forwarded Identity](#forwarded-identity), and the token a server exchanges to call
other servers, described under [Calls Between Servers](#calls-between-servers).

## Standards And Protocols

| Standard | Profile |
|---|---|
| OAuth 2.0 Dynamic Client Registration, RFC 7591 | Registration request and response. The installation's registration secret is the initial access token. A server registers `jwks`, `token_endpoint_auth_method` `private_key_jwt`, `grant_types` holding only `urn:ietf:params:oauth:grant-type:token-exchange` when it declares uses and empty otherwise, empty `response_types`, its slug as `software_id`, its build as `software_version`, a `software_statement` it signs itself, and Veoveo members for its declarations. Errors use the §3.2.2 codes. |
| OAuth 2.0 Dynamic Client Registration Management, RFC 7592 | Read (`GET`), update (`PUT`) and delete (`DELETE`) at `registration_client_uri` with the registration access token. An update replaces the registered metadata. |
| OAuth 2.0, RFC 6749 §8.2 | Registration members without a standard equivalent take the `veoveo_` vendor prefix in snake_case. |
| Bearer tokens, RFC 6750 | Initial access token, registration access token and forwarded access token travel in `Authorization: Bearer`. Every 401 carries `WWW-Authenticate: Bearer`, with `error="invalid_token"` once a token was sent. |
| JWT, RFC 7519, with JWS, RFC 7515, and EdDSA over Ed25519, RFC 8037 | Software statements and forwarded access tokens. `alg` is `EdDSA`; no other algorithm verifies. Times in OAuth documents are NumericDate seconds. |
| JWK and JWK Set, RFC 7517, and JWK Thumbprint, RFC 7638 | Ed25519 `OKP` keys with `kid`, `alg: EdDSA` and `use: sig`. Every `kid`, a server's or the Gateway's, is the key's RFC 7638 thumbprint. |
| JWT assertions, RFC 7523 §3 | Claim rules for the software statement: `iss` and `sub` name the server, `aud` is the URL the request is sent to, `jti` is a UUIDv7, and the lifetime is at most 60 seconds. The client assertion of a token exchange (§2.2) follows the same rules with `iss` and `sub` naming the registration's client identifier and `aud` the token endpoint. |
| JWT access tokens, RFC 9068 | The forwarded identity: `typ: at+jwt`, `iss` is the Gateway's HTTPS issuer URL, `aud` is the target server's resource URL, `client_id` names the OAuth client the caller used, `jti` is a UUIDv7, and `scope`, `groups` and `roles` restate the caller. A server, as resource server, takes the verification keys from the issuer's metadata `jwks_uri` as §4 describes. |
| Resource indicators, RFC 8707 | A forwarded token's `aud` is an absolute URL: the target server's registered MCP endpoint. |
| OAuth 2.0 Token Exchange, RFC 8693 | A server calls other servers with the §2.1 grant, presenting the forwarded token it received or one of its Tasks as the subject, the profile's MCP endpoint as `resource` and its client assertion; `actor_token`, `audience`, `scope` and `requested_token_type` are not used. The `act` claim (§4.1) of a forwarded token names the Gateway as the party acting for the caller, with at most one prior actor: the calling server. |
| Rich Authorization Requests, RFC 9396 | `authorization_details` carries one entry of type `https://veoveo.ai/oauth/permissions` whose `actions` are the granted permissions, in forwarded and exchanged tokens. A token exchange requests permissions in the same form (§6.2), and its response returns the granted entry (§7). |
| Authorization server metadata, RFC 8414 | A server reads `issuer`, `registration_endpoint`, `token_endpoint` and `jwks_uri` from the Gateway's well-known metadata. The issuer is the forwarded tokens' `iss`, and the JWK Set at `jwks_uri` holds the keys that verify them. |
| MCP `2026-07-28` | A server advertises the declarations in Discover under `capabilities.extensions`, with camelCase members. |
| JSON Schema 2020-12 | Schemas for every request, response and declaration are generated from the Rust types by Schemars 1.2. |
| PostgreSQL connection URIs and role names | `veoveo_database` names the installation database as a `postgres://` URL and the server's login as an unquoted PostgreSQL role name. The [database design](https://github.com/VeoveoAI/veoveo-core/blob/60b65e5c2af221bd92f814c98e7558a088413920/docs/DATABASE.md) describes the roles and schemas. |

Every Veoveo addition uses the extension point of the standard it extends:

| Extension point | Names |
|---|---|
| RFC 7591 client metadata, RFC 6749 §8.2 vendor prefix | `veoveo_uri_scheme`, `veoveo_mcp_endpoint`, `veoveo_authorization` |
| RFC 7591 response members, RFC 6749 §8.2 vendor prefix | `veoveo_access_tokens`, `veoveo_database` |
| RFC 9396 authorization details type | `https://veoveo.ai/oauth/permissions` |
| RFC 8693 token type URI | `https://veoveo.ai/token-types/task` |
| JWT public claims, RFC 7519 §4.2 collision-resistant names | `https://veoveo.ai/claims/invocation` |
| MCP `capabilities.extensions` | `ai.veoveo/registration`, `ai.veoveo/authorization` |

Following RFC 7591, a receiver ignores client metadata it does not understand. Each
Veoveo member admits only its own declared fields and refuses unknown ones. OAuth
documents spell members in snake_case, and MCP advertisements in camelCase.

## Declarations

### Identity

A server registers its slug as `software_id`, its build as `software_version`, the URI
scheme of its resources as `veoveo_uri_scheme` and the URL where the Gateway reaches
its MCP endpoint as `veoveo_mcp_endpoint`. MCP Discover advertises the same values as
`ai.veoveo/registration`:

```json
{
  "slug": "time",
  "uriScheme": "time",
  "version": "0.1.0",
  "endpoint": "https://time.internal.example/mcp"
}
```

The version holds 1 to 64 ASCII letters, digits, dots, underscores, plus signs or
hyphens. The endpoint uses `https`, or `http` on a local host as the
[conventions](https://github.com/VeoveoAI/veoveo-core/blob/60b65e5c2af221bd92f814c98e7558a088413920/docs/CONVENTIONS.md#http-paths) allow, names a host and carries no
credentials, query or fragment.

### Authorization

The server's permissions, the permission each tool, resource, resource template and
prompt requires, and the permissions it uses on other servers. The registration member
`veoveo_authorization` spells it in snake_case:

```json
{
  "permissions": [
    { "permission": "time.read", "resource": "time.authority", "access": "read" },
    { "permission": "time.events.write", "resource": "time.event", "access": "write" },
    { "permission": "time.admin", "resource": "time.authority", "access": "write", "risks": ["administer"] }
  ],
  "requirements": {
    "tools": { "resolve_time": "time.read", "create_temporal_event": "time.events.write" },
    "resource_templates": { "time://events/{event}": "time.read" },
    "prompts": { "plan_window": "time.read" }
  },
  "uses": ["artifact.artifacts.read"]
}
```

MCP Discover advertises the same declaration as `ai.veoveo/authorization`, where the
requirements member is `resourceTemplates`.

`access` is exactly one of `read` or `write`. `risks` holds any of `sensitive`,
`destructive`, `billed`, `physical` and `administer`, and an empty set is omitted.
A server builds the declaration from a closed Rust enum that implements
`PermissionDefinition`, and `AuthorizationBuilder` derives every entry from that enum.
The same requirement table drives the server's own handler checks.

`uses` lists the permissions of other servers that this server calls with. A call to
another server carries at most these permissions. `AuthorizationBuilder::uses`
takes a variant of the provider's own permission enum, which the provider's `contract`
feature exports, so a consumer names no permission as text. The member is omitted when
the server calls no other server. A provider need not be registered when the consumer
registers.

The Gateway rejects a declaration when any of these hold:

- a permission or resource type does not start with the server's slug and a dot;
- a permission is declared twice, or a requirement names an undeclared permission;
- a resource or template URI uses neither the server's scheme nor `ui://{slug}/`;
- a template uses an RFC 6570 operator, since only simple `{variable}` expressions
  match concrete URIs;
- a prompt requires a `write` permission;
- a tool annotated `readOnlyHint` requires a `write` permission, or a tool annotated
  `destructiveHint` requires a permission without the `destructive` risk;
- a use names a permission in the server's own namespace, or names one permission
  twice.

A target the declaration omits is denied. An exact resource entry takes precedence
over templates, and a URI matched by templates that require different permissions
requires none and is denied.

Discover advertises the identity and authorization declarations in full, so the
Gateway compares what a server registered with what it serves.

## Namespace Claims

The first registration of a slug claims its namespace and pins the server's public
key to it. For slug `time` the namespace is the slug itself, the `time://` scheme, the
`ui://time/` App prefix, the database schema `time` with its group role `server_time`,
and permissions and resource types under `time.`. Every later registration of that slug,
including a new server version, must be signed by the pinned key. A registration signed
by another key is rejected with `unapproved_software_statement`. Releasing a namespace
is an administrative action outside this protocol.

## Registration

The server discovers the endpoint from the Gateway's RFC 8414 metadata at
`/.well-known/oauth-authorization-server`, then sends:

```http
POST /oauth/register HTTP/1.1
Authorization: Bearer <installation registration secret>
Content-Type: application/json

{
  "jwks": { "keys": [{ "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
                       "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "alg": "EdDSA", "use": "sig" }] },
  "token_endpoint_auth_method": "private_key_jwt",
  "grant_types": [],
  "response_types": [],
  "software_id": "time",
  "software_version": "0.1.0",
  "veoveo_uri_scheme": "time",
  "veoveo_mcp_endpoint": "https://time.internal.example/mcp",
  "veoveo_authorization": { "permissions": [], "requirements": {} },
  "software_statement": "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6Ii4uLiJ9.eyIuLi4iOiIuLi4ifQ.c2ln"
}
```

In an OpenShell environment the server holds a placeholder for the registration secret,
and the proxy substitutes the real value only on requests to the registration endpoint.

`jwks` holds exactly one key, and its `kid` is the key's RFC 7638 thumbprint. The
Gateway admits a request in this order and stops at the first failure:

1. The bearer is the installation's registration secret, or the answer is HTTP 401
   with `WWW-Authenticate: Bearer`.
2. The body decodes into `ClientRegistrationRequest`.
3. `ClientMetadata::check` passes: empty grant and response types and the
   authorization declaration within the namespace.
4. `statement::verify_statement` passes against the pinned key, if the namespace has
   one, and the registration endpoint URL.

The Gateway then pins the namespace, calls `kernel.ensure_server` for the server's
group role and schema, creates a login for this registration with
`kernel.create_server_login` and answers HTTP 201:

```json
{
  "client_id": "time-0b6f",
  "client_id_issued_at": 1791763200,
  "registration_access_token": "<opaque>",
  "registration_client_uri": "https://gateway.example/oauth/register/time-0b6f",
  "jwks": { "keys": ["..."] },
  "token_endpoint_auth_method": "private_key_jwt",
  "grant_types": [],
  "response_types": [],
  "software_id": "time",
  "software_version": "0.1.0",
  "veoveo_uri_scheme": "time",
  "veoveo_mcp_endpoint": "https://time.internal.example/mcp",
  "veoveo_authorization": { "...": "..." },
  "software_statement": "<as sent>",
  "veoveo_access_tokens": {
    "issuer": "https://gateway.example",
    "jwks": { "keys": [{ "kty": "OKP", "crv": "Ed25519", "x": "...", "kid": "<thumbprint>", "alg": "EdDSA", "use": "sig" }] },
    "resource": "https://time.internal.example/mcp"
  },
  "veoveo_database": {
    "url": "postgres://postgres.internal.example:5432/veoveo",
    "username": "time_0b6f",
    "password": "<secret>"
  }
}
```

The response repeats the registered metadata unchanged, and the registration client
rejects a response that differs from what it sent. `veoveo_database` carries the
installation database URL, which names a host and one database and holds no
credentials, and the login role created for this registration with its password. The
role name is a lowercase letter or underscore followed by at most 62 lowercase letters,
digits or underscores. The login is a member of the server's group role
`server_<server>`, which owns the schema named after the slug with `-` replaced by
`_`. The server runs its own migrations in that schema after `SET ROLE
server_<server>`; deregistration drops the login. `veoveo_access_tokens` names the issuer, keys
and audience of the access tokens the Gateway forwards to this server. The issuer is
the Gateway's issuer URL, the value its RFC 8414 metadata announces, and the
audience is the registered MCP endpoint. The keys are the server's initial set, which
[key refresh](#key-refresh) replaces when the Gateway rotates its key. Every URL here
uses `https`, or `http` on a local host.

### Software Statement

The software statement proves that the registering process holds the private key in
`jwks` and binds that key to every declaration. It is a JWT whose header carries
`alg: EdDSA` and the key's thumbprint as `kid`, and whose claims are the request's
client metadata together with:

| Claim | Value |
|---|---|
| `iss`, `sub` | the server's slug |
| `aud` | the URL the request is sent to: the registration endpoint, or `registration_client_uri` for an update |
| `iat`, `exp` | issue and expiry time, at most 60 seconds apart |
| `jti` | a fresh UUIDv7 |

The Gateway verifies the signature with the key in `jwks`, requires that key to equal
the pinned key when the namespace has one, and requires the statement's metadata to
equal the request's. It tolerates 5 seconds of clock difference at either end of the
lifetime. A failure is `invalid_software_statement`, except a foreign key for a pinned
namespace, which is `unapproved_software_statement`.

## Update

An RFC 7592 update replaces the registered metadata:

```http
PUT /oauth/register/time-0b6f HTTP/1.1
Authorization: Bearer <registration access token>
Content-Type: application/json

{ "client_id": "time-0b6f", "jwks": { "...": "..." }, "...": "...", "software_statement": "<signed for this URL>" }
```

The Gateway admits an update with the same steps as a registration, using the
registration access token in place of the initial access token, and answers HTTP 200
with the client information. An update with changed declarations takes the full
admission of a new registration under the pinned key; an update that repeats the
registered metadata changes nothing. The Gateway may rotate the registration access
token, and the client adopts the returned one. An update never extends a registration.

## Liveness And Removal

A registration lasts until the server deletes it or its process ends. The server
connects with the login its registration granted and holds a kernel instance on that
login while it serves, as the [database design](https://github.com/VeoveoAI/veoveo-core/blob/60b65e5c2af221bd92f814c98e7558a088413920/docs/DATABASE.md#process-instances)
describes. The Gateway removes a registration whose login has held no live instance for
the installation's grace period, 60 seconds by default, and drops the login. It then
answers the registration access token with HTTP 401.

`GET` at `registration_client_uri` returns the current client information with HTTP
200. `DELETE` removes the registration, drops its login and answers HTTP 204. A server
deletes its registration when it shuts down, and the registration client treats HTTP
401 on delete as already removed.

## Errors

| Condition | HTTP | Body |
|---|---|---|
| Missing or wrong initial access token or registration access token | 401 with `WWW-Authenticate: Bearer` | none |
| Malformed body or a declaration outside the namespace | 400 | `invalid_client_metadata` |
| Malformed, stale, misaddressed or mismatched software statement | 400 | `invalid_software_statement` |
| Namespace pinned to another key | 400 | `unapproved_software_statement` |

An error body is an RFC 7591 §3.2.2 object. `error_description` states the exact
reason and names declared identifiers only, never request contents or secrets.

## Forwarded Identity

The Gateway authorizes each request with installation roles over the target server's
declared permissions, as the policy evaluator in `veoveo-service-gateway`
describes. It then forwards the request with a fresh RFC 9068 access token signed by
one of the keys in `veoveo_access_tokens`:

```json
{ "typ": "at+jwt", "alg": "EdDSA", "kid": "<RFC 7638 thumbprint>" }
```

```json
{
  "iss": "https://gateway.example",
  "sub": "https://idp.example#alice",
  "aud": "https://time.internal.example/mcp",
  "client_id": "console",
  "act": { "sub": "https://gateway.example" },
  "iat": 1791763200,
  "nbf": 1791763200,
  "exp": 1791763260,
  "jti": "0199d4a1-7c00-7b3e-9f2a-6c1d2e3f4a5b",
  "scope": "time:use",
  "groups": ["tenant-members"],
  "authorization_details": [
    { "type": "https://veoveo.ai/oauth/permissions", "actions": ["time.events.write", "time.read"] }
  ],
  "https://veoveo.ai/claims/invocation": {
    "profile": "default",
    "server": "time",
    "actor": { "id": "https://idp.example#alice", "kind": "user", "...": "..." },
    "authority": { "work_context": "development", "tenant": "dev", "membership": "contributor", "...": "..." },
    "request_context": { "...": "..." }
  }
}
```

`sub` is the caller and `client_id` the OAuth client the caller used to reach the
Gateway. The Gateway is the party acting for the caller, so `act` names it by its
issuer URL. `aud` is the target server's resource URL, its registered MCP endpoint, and
the invocation claim names the server's slug. The invocation claim also carries the
profile, the actor, the invocation authority with its tenant, Work Context and
membership, and the signed request context when one exists. RFC 7519 §4.2 asks for
collision-resistant names for claims outside the IANA registry, and every registered
server reads the invocation claim, so it uses a URI in the `veoveo.ai` domain rather
than a private name. `authorization_details` is omitted when nothing is granted. A
server verifies the token and rejects it when:

- `typ` is neither `at+jwt` nor `application/at+jwt`, or `alg` is not `EdDSA`;
- `kid` names no key the server holds after [key refresh](#key-refresh), or the
  signature fails;
- `iss` differs from the registered issuer, `aud` differs from the registered
  resource, the invocation claim names another server, or `exp` and `nbf` place the
  current time outside the token's lifetime, with no leeway;
- `jti` is not a UUIDv7;
- `act` is missing, names another party than `iss`, or carries anything but `sub` and
  one prior actor;
- `act` names a prior actor without the invocation claim's `acting_server`, or the
  reverse, or the invocation claim names an `acting_task` without an `acting_server`;
- `sub`, `groups`, `roles` and `scope` disagree with the embedded actor, or
  `client_id` differs from the client in the request context;
- the token carries more than one authorization details entry, or a granted
  permission lies outside the server's namespace;
- the request context disagrees with the actor or authority, or an undelegated token
  outlives the caller's own access token.

A call that another registered server makes for its caller carries a delegated token.
The caller stays `sub`, `client_id` and the actor, and the Gateway acts with the calling
server as the prior actor of RFC 8693 §4.1:

```json
{
  "act": { "sub": "https://gateway.example", "act": { "sub": "media-4f2c9a01b3d8e7" } },
  "https://veoveo.ai/claims/invocation": {
    "server": "artifact",
    "acting_server": "media",
    "acting_task": "0199d4a1-7c00-7b3e-9f2a-6c1d2e3f4a5b",
    "...": "..."
  }
}
```

The prior actor's `sub` is the calling server's registration client identifier, and
`acting_server` its slug. When the calling server called from one of its Tasks, which
its exchange named as the subject, `acting_task` is that Task's identifier as the
calling server issued it, and it appears only beside `acting_server`.
`GatewayInternalIdentity::acting_server` and `GatewayInternalIdentity::acting_task`
return them, and the receiving server records both as provenance. A delegated token ends with the
exchanged token that carried the call, which the Gateway bounds by the caller's token
or by the Task the call serves, so the verifier does not compare it with the caller's
original access token.

A rejected request receives HTTP 401 with `WWW-Authenticate: Bearer`, adding
`error="invalid_token"` when a token was sent. An ordinary request's token lives at
most 60 seconds, and a token that opens `subscriptions/listen` at most 15 minutes.
Both end no later than the caller's own access token.

The server checks the required permission again with
`GatewayInternalIdentity::grants`. Per-object rules, such as an owner-only event,
stay with the server. `veoveo-sdk-rust` issues these tokens with
`GatewayInternalTokenIssuer::issue` from a `ForwardedRequest`.
`ForwardedTokenVerifier::new` verifies them against the issuer, keys and resource in
`veoveo_access_tokens` and refreshes the keys through the HTTP client it is given. The
hosted runtime takes the same values through `HostedServerBuilder::internal_trust`.

### Key Refresh

The Gateway publishes the keys that sign forwarded tokens in the JWK Set its RFC 8414
metadata names as `jwks_uri`. A server keeps the keys of its registration response in
memory as its initial set. When a token's `kid` names a key the server does not hold,
the server, as RFC 9068 §4 describes:

1. reads `{issuer origin}/.well-known/oauth-authorization-server{issuer path}`, the
   metadata URL RFC 8414 §3.1 derives from the issuer in `veoveo_access_tokens`, and
   requires its `issuer` to equal that issuer (§3.3);
2. reads the JWK Set at the metadata's `jwks_uri`, which uses `https`, or `http` on a
   local host, and carries no credentials, query or fragment;
3. keeps the set's Ed25519 keys in the shape of `veoveo_access_tokens`, each named by
   its RFC 7638 thumbprint, and ignores every other entry, as RFC 7517 §5 asks;
4. replaces its keys with that set when the set holds the token's key, and verifies the
   token again.

The server fetches at most once every 30 seconds, whatever the outcome, and concurrent
requests wait for one fetch. Each request has a 5-second timeout and a 64 KiB body
limit, follows no redirect and expects HTTP 200. A failed fetch, or a set without the
token's key, leaves the keys unchanged and rejects the token. A token with another
defect, such as a known key with a bad signature, never causes a fetch. The Gateway
publishes a new key before it signs with it and keeps a retired key in the set until
the tokens it signed expire.

## Calls Between Servers

A server calls another server's tools, reads its resources and subscribes to their
updates through the Gateway for its own caller. It declares the permissions it calls
with in `uses`, exchanges a token for each provider, and sends each request to the
profile the caller reached it through.

```text
caller -> Gateway -> media            forwarded token, aud media
media  -> Gateway /oauth/token        token exchange: subject, resource, permissions
media  -> Gateway /mcp/{profile}      exchanged token, aud the profile
Gateway -> artifact                   delegated forwarded token, act gateway, then media
```

The exchange is one RFC 8693 request to the Gateway's `token_endpoint`:

| Parameter | Value |
|---|---|
| `grant_type` | `urn:ietf:params:oauth:grant-type:token-exchange` |
| `client_id`, `client_assertion_type`, `client_assertion` | The registration's client identifier and an RFC 7523 assertion signed with the server's pinned key |
| `subject_token`, `subject_token_type` | The forwarded token the server received for the current call, as `urn:ietf:params:oauth:token-type:access_token`; or the identifier of one of the server's own Tasks, as `https://veoveo.ai/token-types/task` |
| `resource` | The profile's MCP endpoint, `{issuer}/mcp/{profile}`, of the profile the subject came through |
| `authorization_details` | Optional: one `https://veoveo.ai/oauth/permissions` entry naming declared uses in one provider's namespace; without it the request names every declared use |

A forwarded token is a subject only for the server it was addressed to, and only when
no other server made that call: an exchanged identity is never exchanged again. A Task
is a subject while it runs, if the call that created it came through the Gateway. The
Gateway records each such Task when the call that created it returns, so a Task that
calls another server at once may reach the Gateway first; the SDK repeats a Task's
exchange that the Gateway refuses as an invalid grant for up to two seconds.

The issued token is an RFC 9068 access token for the profile: `iss` is the Gateway's
issuer, `aud` the profile's MCP endpoint, `sub` the caller, and `client_id` and `act`
the calling server's registration. Its invocation claim holds the caller as the
Gateway authenticated it, with the request context, the acting server's slug and, for
a Task subject, the Task. It carries the requested permissions that the server
declared and the caller holds when the Gateway issues it. It lives no longer than the
subject token, and at most five minutes for a Task, whose delegation ends when the
Task ends even if the token has not expired. The response is the RFC 8693 §2.2.1
document with `issued_token_type` `urn:ietf:params:oauth:token-type:access_token`,
`token_type` `Bearer`, `expires_in` and the granted `authorization_details`.

| Refusal | `error` |
|---|---|
| A malformed request or a missing subject | `invalid_request` |
| No registration, or an assertion that does not verify or is replayed | `invalid_client` |
| A registration without the token exchange grant | `unauthorized_client` |
| A subject that is another server's, delegated, expired or an ended Task | `invalid_grant` |
| A `resource` that is not the subject's profile, or a permission no registered server provides, named in `error_description` | `invalid_target` |
| A requested permission the server did not declare in `uses` | `invalid_authorization_details` |

At the profile the Gateway admits the exchanged token itself, whatever authorization
servers the profile names. The permissions of each request, whether a tool call, a
resource read or a subscription, are the caller's role-granted permissions in the
target's namespace narrowed to the token's, and the target receives the [delegated
forwarded token](#forwarded-identity), which names the calling server's Task when the
token's subject was one.

`ServerCalls` holds what a server calls with: its client identifier, key, `uses` and
the token endpoint, which `ServerCalls::discover` reads from the issuer's metadata.
`HostedServerBuilder::calls` gives it to the handlers, where `ctx.client()` through
`ClientContext` returns a `GatewayClient` bound to the request's caller, and
`ServerCalls::for_task` returns one bound to a Task. A `GatewayClient` exchanges a token
on its first call to a provider, caches it per binding and provider until five seconds
before it expires, and replaces a token the Gateway stops admitting once. The client
API is described in the [crate design](../../DESIGN.md#calls-to-other-servers).

## Registration Client

The `client` feature provides the client every server runs:

```rust
let key = KeyDirectory::new(&args.key_directory).load_or_generate()?;
let endpoint = RegistrationClient::discover(&http, &gateway_issuer).await?;
let metadata = ClientMetadata::new(
    *key.public_key(),
    registration,
    AuthorizationBuilder::<TimePermission>::new()
        .tool(LocalToolName::parse("resolve_time")?, TimePermission::Read)
        .build(&slug, &scheme)?,
)?;
let client = RegistrationClient::new(http, endpoint, registration_secret, key, metadata)?;
let registration = client.register().await?;
let database = registration.database();
let server = HostedServer::for_domain::<TimeDomain>()
    .deployment(&deployment, false)?
    .internal_trust(registration.access_tokens(), http.clone())?;
tokio::spawn(async move { client.maintain(registration, shutdown_signal()).await });
```

`KeyDirectory` generates the Ed25519 key on first start and stores it as PKCS#8 in
`server-key.p8`, readable only by its owner and written atomically. Restarts reuse the
key, so the server keeps its namespace. `RegistrationClient::maintain` waits for the
shutdown future and then deletes the registration; `update` sends an RFC 7592 update
with the client's metadata. The caller supplies the `reqwest::Client` and installs a
rustls crypto provider before reaching the Gateway over HTTPS.

## Crate Features

| Feature of `veoveo-sdk-rust` | Adds to this module |
|---|---|
| `contract` | Declarations, the OAuth request and response types, the token exchange wire types, `PermissionsDetail`, JWK types and their schemas |
| `server` | `ServerKeyPair`, the software statement and client assertion on AWS-LC, `KeyDirectory` and `RegistrationClient` on reqwest, and advertising and reading the declarations in MCP capabilities with the tool annotation check |

## Qualification

`tests/wire.rs` fixes the wire form of every declaration, request, response, error and
authorization detail, and validates the examples against the generated JSON Schemas.
It checks the RFC 7638 thumbprint against RFC 8037 Appendix A. `tests/client.rs` runs
the registration client against an in-process endpoint that applies the admission
steps above: registration, read, update with token rotation, an update of a removed
registration, removal at shutdown, the pinned key and the initial access token. The software statement
tests cover the pinned key, metadata, audience and lifetime. The forwarded token
tests in `veoveo-sdk-rust` cover the token type, issuer, audience, actor, `jti`,
the single authorization detail, the namespace of granted permissions and agreement
between standard and Veoveo claims. Its key refresh tests run an issuer on a loopback
port: a rotated key, one fetch for concurrent requests, the 30-second interval, a set
without the token's key, a foreign issuer, a missing or remote plain-HTTP `jwks_uri`, a
redirect, an oversized document and an unreachable issuer.
