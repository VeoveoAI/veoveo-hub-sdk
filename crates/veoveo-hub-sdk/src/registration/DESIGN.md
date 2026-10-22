# Server Registration Protocol

This document is the normative protocol between an MCP server and the Veoveo Hub.
A server registers itself, declares what it owns and which permission each of its
targets requires, and keeps its registration while its process holds a live kernel
instance on the database login the registration grants. The Hub forwards each admitted request with the caller's granted permissions, and the server
checks them again. Neither the Hub nor the installation configuration names a
server, so every server enters the catalog through this protocol.

The `registration` module of `veoveo-hub-sdk` holds the typed declarations, the OAuth
request and response types, the software statement, the client assertion, the
granted-permissions detail and the registration client that servers run. The
`internal_auth` module issues and verifies the forwarded access token, described under
[Forwarded Identity](#forwarded-identity), and the token a server exchanges to call
other servers, described under [Calls Between Servers](#calls-between-servers).

## Execution And Deployment Contract

Every declaration identifies an admitted `DeploymentId`, separate from its logical
slug. MCP registration exposes `deployment`; RFC 7591 metadata carries the same
identity as required `veoveo_deployment_id`. A namespace's private key and the global
initial registration secret are not credentials to distribute to user overrides.
The SDK wire identity does not authorize a new key: namespace-owner admission remains
in place until the Hub explicitly admits a separately keyed deployment. The Hub
must retain registrations by deployment rather than let the newest logical slug
silently replace another environment's selection.

The registration response's `veoveo_access_tokens.deployment` must equal the declared
deployment. Verifiers derive the expected target from that admitted trust and reject
another deployment's token even for the same slug and resource URL. Signed invocation
claims carry that target separately from `HubRequestContext.execution`; downstream
calls retain the environment/composition capture rather than select the latest map.
The runtime-free resolved-composition and HTTP selector contract is documented in
[`docs/CONVENTIONS.md`](../../../../docs/CONVENTIONS.md#execution-selection-and-deployment-identity).
Hub admission, per-request routing and retained Task resolution remain Hub-owned work;
these SDK types do not implement that routing by themselves.

`veoveo_database.binding` explicitly identifies the admitted storage contract. The
currently supported `shared_logical_schema` binding names the registered logical
server and grants shared data under the existing native runtime/ACL contract. The
client rejects another logical server's binding. The Hub must authorize use of this
binding before issuing a login; permission to edit an environment is insufficient.
This does not provide owner-isolated schemas or cloned data. Unsupported isolation
must be rejected, and a missing domain-storage grant does not make a runtime's
required kernel login optional.

## Standards And Protocols

| Standard | Profile |
|---|---|
| OAuth 2.0 Dynamic Client Registration, RFC 7591 | Registration request and response. The installation's registration secret is the initial access token. A server registers `jwks`, `token_endpoint_auth_method` `private_key_jwt`, `grant_types` holding only `urn:ietf:params:oauth:grant-type:token-exchange` when it declares uses or calls to the Hub and empty otherwise, empty `response_types`, its slug as `software_id`, its build as `software_version`, a `software_statement` it signs itself, and Veoveo members for its declarations. Errors use the §3.2.2 codes. |
| OAuth 2.0 Dynamic Client Registration Management, RFC 7592 | Read (`GET`), update (`PUT`) and delete (`DELETE`) at `registration_client_uri` with the registration access token. An update replaces the registered metadata. |
| OAuth 2.0, RFC 6749 §8.2 | Registration members without a standard equivalent take the `veoveo_` vendor prefix in snake_case. |
| Bearer tokens, RFC 6750 | Initial access token, registration access token and forwarded access token travel in `Authorization: Bearer`. Every 401 carries `WWW-Authenticate: Bearer`, with `error="invalid_token"` once a token was sent. |
| JWT, RFC 7519, with JWS, RFC 7515, and EdDSA over Ed25519, RFC 8037 | Software statements and forwarded access tokens. `alg` is `EdDSA`; no other algorithm verifies. Times in OAuth documents are NumericDate seconds. |
| JWK and JWK Set, RFC 7517, and JWK Thumbprint, RFC 7638 | Ed25519 `OKP` keys with `kid`, `alg: EdDSA` and `use: sig`. Every `kid`, a server's or the Hub's, is the key's RFC 7638 thumbprint. |
| JWT assertions, RFC 7523 §3 | Claim rules for the software statement: `iss` and `sub` name the server, `aud` is the URL the request is sent to, `jti` is a UUIDv7, and the lifetime is at most 60 seconds. The client assertion of a token exchange (§2.2) follows the same rules with `iss` and `sub` naming the registration's client identifier and `aud` the token endpoint. |
| JWT access tokens, RFC 9068 | The forwarded identity: `typ: at+jwt`, `iss` is the Hub's HTTPS issuer URL, `aud` is the target server's resource URL, `client_id` names the OAuth client the caller used, `jti` is a UUIDv7, and `scope`, `groups` and `roles` restate the caller. A server, as resource server, takes the verification keys from the issuer's metadata `jwks_uri` as §4 describes. |
| Resource indicators, RFC 8707 | A forwarded token's `aud` is an absolute URL: the target server's registered MCP endpoint. |
| OAuth 2.0 Token Exchange, RFC 8693 | A server calls other servers with the §2.1 grant, presenting the forwarded token it received or one of its Tasks as the subject, the profile's MCP endpoint as `resource` and its client assertion; `actor_token`, `audience`, `scope` and `requested_token_type` are not used. The `act` claim (§4.1) of a forwarded token names the Hub as the party acting for the caller, followed by a bounded nested chain of registered server actors. |
| Rich Authorization Requests, RFC 9396 | `authorization_details` carries one entry of type `https://veoveo.ai/oauth/permissions` whose `actions` are the granted permissions, in forwarded and exchanged tokens. A token exchange requests permissions in the same form (§6.2), and its response returns the granted entry (§7). |
| Authorization server metadata, RFC 8414 | A server reads `issuer`, `registration_endpoint`, `token_endpoint` and `jwks_uri` from the Hub's well-known metadata. The issuer is the forwarded tokens' `iss`, and the JWK Set at `jwks_uri` holds the keys that verify them. |
| MCP `2026-07-28` | A server advertises the declarations in Discover under `capabilities.extensions`, with camelCase members. |
| JSON Schema 2020-12 | Schemas for every request, response and declaration are generated from the Rust types by Schemars 1.2. |
| PostgreSQL connection URIs and role names | `veoveo_database` names the installation database as a `postgres://` URL and the server's login as an unquoted PostgreSQL role name. The [database design](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/docs/DATABASE.md) describes the roles and schemas. |

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
scheme of its resources as `veoveo_uri_scheme` and the URL where the Hub reaches
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
[conventions](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/docs/CONVENTIONS.md#http-paths) allow, names a host and carries no
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
    "tools": {
      "resolve_time": {"kind":"permission", "permission":"time.read"},
      "create_event": {"kind":"permission", "permission":"time.events.write"}
    },
    "resource_templates": { "time://events/{event}": {"kind":"permission", "permission":"time.read"} },
    "prompts": { "plan_window": {"kind":"permission", "permission":"time.read"} }
  },
  "uses": ["artifact.artifacts.read"]
}
```

MCP Discover advertises the same declaration as `ai.veoveo/authorization`, where the
requirements member is `resourceTemplates`.

`access` is exactly one of `read` or `write`. `risks` holds any of `sensitive`,
`destructive`, `billed`, `physical` and `administer`, and an empty set is omitted.
A server supplies its closed `PermissionDefinition` enum to required `RoutePolicy`
steps while constructing typed `Routes`. The ready table derives this declaration,
discovery, dispatch admission and tool annotation hints. There is no independent
authorization authoring table. An explicitly `Authenticated` route serializes as
`{"kind":"authenticated"}` and requires valid current admission without an additional
permission; an absent route remains undeclared.

`uses` lists the permissions of other servers that this server calls with. A call to
another server carries at most these permissions. `RoutesBuilder::uses` takes the
native `PermissionName` from the provider's exported permission enum, for example
`.uses(ArtifactPermission::ArtifactsRead.name())`. The member is omitted when
the server calls no other server. A provider need not be registered when the consumer
registers.

`calls_hub` (`callsHub` in Discover) is `true` when the server calls the Hub's own
routes for its caller, which `RoutesBuilder::calls_hub` declares. The member is
omitted otherwise. A server that declares uses or calls to the Hub registers the
token exchange grant.

The Hub rejects a declaration when any of these hold:

- a permission or resource type does not start with the server's slug and a dot;
- a permission is declared twice, or a requirement names an undeclared permission;
- a resource or template URI uses neither the server's scheme nor `ui://{slug}/`;
- a template is outside the native resource route profile: a literal scheme and
  authority, literal or whole `{variable}` path segments, an optional terminal
  `{+tail}` and one optional terminal `{?name,...}` query expansion. Other operators,
  variable authorities, mixed segments, modifiers and fixed queries are refused;
- a prompt requires a `write` permission;
- a use names a permission in the server's own namespace, or names one permission
  twice.

A target the declaration omits is denied. An exact resource entry takes precedence
over templates, and a URI matched by templates that require different permissions
requires none and is denied. Collection templates match their root with no query
and pages containing any subset of the declared query names, in any order. Unknown
query names, empty query strings and duplicate decoded names (including encoded
aliases) are refused. Scalar captures consume one path segment; a terminal reserved
tail consumes the remaining segments, including zero, as the native route machinery
does. Domain address types still validate captured values. This authorization route
profile is distinct from Core's lexical resource policy selectors.

Discover advertises the identity and authorization declarations in full, so the
Hub compares what a server registered with what it serves.

## Namespace Claims

The first registration of a slug claims its namespace and pins the server's public
key to it. For slug `time` the namespace is the slug itself, the `time://` scheme, the
`ui://time/` App prefix, the database schema `time` with its group role `server_time`,
and permissions and resource types under `time.`. Every later registration of that slug,
including a new server version, must be signed by the pinned key. A registration signed
by another key is rejected with `unapproved_software_statement`. Releasing a namespace
is an administrative action outside this protocol.

## Registration

The server discovers the endpoint from the Hub's RFC 8414 metadata at
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

In a local installation's environment the server holds a placeholder for the
registration secret, and the installation's edge replaces it with the real value only on
`POST /oauth/register`.

`jwks` holds exactly one key, and its `kid` is the key's RFC 7638 thumbprint. The
Hub admits a request in this order and stops at the first failure:

1. The bearer is the installation's registration secret, or the answer is HTTP 401
   with `WWW-Authenticate: Bearer`.
2. The body decodes into `ClientRegistrationRequest`.
3. `ClientMetadata::check` passes: empty grant and response types and the
   authorization declaration within the namespace.
4. `statement::verify_statement` passes against the pinned key, if the namespace has
   one, and the registration endpoint URL.

The Hub then pins the namespace, calls `kernel.ensure_server` for the server's
group role and schema, creates a login for this registration with
`kernel.create_server_login` and answers HTTP 201:

```json
{
  "client_id": "time-0b6f",
  "client_id_issued_at": 1791763200,
  "registration_access_token": "<opaque>",
  "registration_client_uri": "https://hub.example/oauth/register/time-0b6f",
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
    "issuer": "https://hub.example",
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
and audience of the access tokens the Hub forwards to this server. The issuer is
the Hub's issuer URL, the value its RFC 8414 metadata announces, and the
audience is the registered MCP endpoint. The keys are the server's initial set, which
[key refresh](#key-refresh) replaces when the Hub rotates its key. Every URL here
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

The Hub verifies the signature with the key in `jwks`, requires that key to equal
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

The Hub admits an update with the same steps as a registration, using the
registration access token in place of the initial access token, and answers HTTP 200
with the client information. An update with changed declarations takes the full
admission of a new registration under the pinned key; an update that repeats the
registered metadata changes nothing. The Hub may rotate the registration access
token, and the client adopts the returned one. An update never extends a registration.

## Liveness And Removal

A registration lasts until the server deletes it or its process ends. The server
connects with the login its registration granted and holds a kernel instance on that
login while it serves, as the [database design](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/docs/DATABASE.md#process-instances)
describes. The Hub removes a registration whose login has held no live instance for
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

The Hub authorizes each request with installation roles over the target server's
declared permissions, as the policy evaluator in `veoveo-hub`
describes. It then forwards the request with a fresh RFC 9068 access token signed by
one of the keys in `veoveo_access_tokens`:

```json
{ "typ": "at+jwt", "alg": "EdDSA", "kid": "<RFC 7638 thumbprint>" }
```

```json
{
  "iss": "https://hub.example",
  "sub": "https://idp.example#alice",
  "aud": "https://time.internal.example/mcp",
  "client_id": "console",
  "act": { "sub": "https://hub.example" },
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
Hub. The Hub is the party acting for the caller, so `act` names it by its
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
- `act` is missing, names another party than `iss`, or any actor carries members
  other than `sub` and an optional nested `act`;
- `act` names a prior actor without the invocation claim's `acting_server`, or the
  reverse, or the invocation claim names an `acting_task` without an `acting_server`;
- `sub`, `groups`, `roles` and `scope` disagree with the embedded actor, or
  `client_id` differs from the client in the request context;
- the token carries more than one authorization details entry, or a granted
  permission lies outside the server's namespace;
- the request context disagrees with the actor or authority, or an undelegated token
  outlives the caller's own access token.

Nested requests additionally carry `delegation_context` in the signed invocation:
`actors`, an ordered list of earlier registered servers and their OAuth client IDs;
optional `task`, the originating Task's `server` and `task`; and `expires_at`, the
inherited request deadline. The list must be nonempty, must exactly match the earlier
RFC 8693 `act` chain, and together with the immediate server may contain at most
**eight registered server actors**, excluding the Hub. A nested token cannot outlive
that deadline. An originating Task belongs to the first actor. A nested request cannot
also claim its immediate actor's own Task, because a Task created by a delegated call
acquires no further delegation. Unknown attribution fields and inconsistent histories
are refused. These constraints are validated when issuing and verifying both exchanged
and forwarded tokens.

A call that another registered server makes for its caller carries a delegated token.
The caller stays `sub`, `client_id` and the actor, and the Hub acts with the calling
server as the prior actor of RFC 8693 §4.1:

```json
{
  "act": { "sub": "https://hub.example", "act": { "sub": "media-4f2c9a01b3d8e7" } },
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
`HubInternalIdentity::acting_server` and `HubInternalIdentity::acting_task`
return them, and the receiving server records both as provenance. A delegated token ends with the
exchanged token that carried the call, which the Hub bounds by the caller's token
or by the Task the call serves, so the verifier does not compare it with the caller's
original access token.

When a second server calls onward, the new immediate server is the outer registered
actor and earlier actors stay nested inside it. The original caller's `sub`, OAuth
client, actor, invocation authority and signed request context remain unchanged.
`HubInternalIdentity::delegating_task` and `ExchangedIdentity::delegating_task` expose
the originating Task constraint independently of the new immediate actor; it is not
misreported as that actor's own Task. The Hub rechecks the originating Task's native
route and session revocation on each further exchange and exchanged-token admission.

A rejected request receives HTTP 401 with `WWW-Authenticate: Bearer`, adding
`error="invalid_token"` when a token was sent. An ordinary request's token lives at
most 60 seconds, and a token that opens `subscriptions/listen` at most 15 minutes.
Both end no later than the caller's own access token.

The server checks the required permission again with
`HubInternalIdentity::grants`. Per-object rules, such as an owner-only event,
stay with the server. `veoveo-hub-sdk` issues these tokens with
`HubInternalTokenIssuer::issue` from a `ForwardedRequest`.
`ForwardedTokenVerifier::new` verifies them against the issuer, keys, deployment and resource in
`veoveo_access_tokens` and refreshes the keys through the HTTP client it is given. The
hosted runtime takes the same values through `HostedServerBuilder::internal_trust`.

### Key Refresh

The Hub publishes the keys that sign forwarded tokens in the JWK Set its RFC 8414
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
defect, such as a known key with a bad signature, never causes a fetch. The Hub
publishes a new key before it signs with it and keeps a retired key in the set until
the tokens it signed expire.

## Calls Between Servers

A server calls another server's tools, reads its resources and subscribes to their
updates through the Hub for its own caller. It declares the permissions it calls
with in `uses`, exchanges a token for each provider, and sends each request to the
profile the caller reached it through.

```text
caller -> Hub -> media            forwarded token, aud media
media  -> Hub /oauth/token        token exchange: subject, resource, permissions
media  -> Hub /mcp/{profile}      exchanged token, aud the profile
Hub -> artifact                   delegated forwarded token, act hub, then media
```

The exchange is one RFC 8693 request to the Hub's `token_endpoint`:

| Parameter | Value |
|---|---|
| `grant_type` | `urn:ietf:params:oauth:grant-type:token-exchange` |
| `client_id`, `client_assertion_type`, `client_assertion` | The registration's client identifier and an RFC 7523 assertion signed with the server's pinned key |
| `subject_token`, `subject_token_type` | The forwarded token the server received for the current call, as `urn:ietf:params:oauth:token-type:access_token`; or the identifier of one of the server's own Tasks, as `https://veoveo.ai/token-types/task` |
| `resource` | The profile's MCP endpoint, `{issuer}/mcp/{profile}`, of the profile the subject came through |
| `authorization_details` | Optional: one `https://veoveo.ai/oauth/permissions` entry naming declared uses in one provider's namespace; without it the request names every declared use |

A forwarded token is a subject only for the server it was addressed to. That server
can continue a delegated request within the bounded actor chain and inherited
deadline. At each hop it authenticates with its own registration and requests its own
declared `uses`; the Hub narrows them to the original caller's currently authorized
provider permissions under the retained signed authority. An exchanged token itself
is not a server-addressed subject. A Task is a subject while it runs, if the call that
created it came directly through the Hub; a delegated child Task acquires no further
delegation. The
Hub records each such Task when the call that created it returns, so a Task that
calls another server at once may reach the Hub first; the SDK repeats a Task's
exchange that the Hub refuses as an invalid grant for up to two seconds.

The issued token is an RFC 9068 access token for the profile: `iss` is the Hub's
issuer, `aud` the profile's MCP endpoint, `sub` the caller, and `client_id` and `act`
the calling server's registration. Its invocation claim holds the caller as the
Hub authenticated it, with the request context, the acting server's slug and, for
a Task subject, the Task. It carries the requested permissions that the server
declared and the caller holds when the Hub issues it. It lives no longer than the
subject token, and at most five minutes for a Task, whose delegation ends when the
Task ends even if the token has not expired. The response is the RFC 8693 §2.2.1
document with `issued_token_type` `urn:ietf:params:oauth:token-type:access_token`,
`token_type` `Bearer`, `expires_in` and the granted `authorization_details`.

| Refusal | `error` |
|---|---|
| A malformed request or a missing subject | `invalid_request` |
| No registration, or an assertion that does not verify or is replayed | `invalid_client` |
| A registration without the token exchange grant | `unauthorized_client` |
| A subject that is another server's, expired, beyond the actor bound, constrained by an ended Task or revoked originating session, or a delegated child Task | `invalid_grant` |
| A `resource` that is not the subject's profile, or a permission no registered server provides, named in `error_description` | `invalid_target` |
| A requested permission the server did not declare in `uses` | `invalid_authorization_details` |

At the profile the Hub admits the exchanged token itself, whatever authorization
servers the profile names. The permissions of each request, whether a tool call, a
resource read or a subscription, are the caller's role-granted permissions in the
target's namespace narrowed to the token's, and the target receives the [delegated
forwarded token](#forwarded-identity), which names the calling server's Task when the
token's subject was one.

A server that declares calls to the Hub exchanges the same subject for the same
`resource` with one `https://veoveo.ai/oauth/permissions` entry whose `actions` is
empty. The issued token carries no permissions; the Hub admits it at its own routes
for the profile as the caller, as it admits any exchanged token, and those routes
decide for the caller.

`ServerCalls` holds what a server calls with: its client identifier, key, `uses` and
the token endpoint, which `ServerCalls::discover` reads from the issuer's metadata.
`HostedServerBuilder::calls` gives it to the handlers, where `ctx.client()` through
`ClientContext` returns a `HubClient` bound to the request's caller, and
`ServerCalls::for_task` returns one bound to a Task. A `HubClient` exchanges a token
on its first call to a provider, caches it per binding and provider until five seconds
before it expires, and replaces a token the Hub stops admitting once. The client
API is described in the [crate design](../../DESIGN.md#calls-to-other-servers).

## Registration Client

The `client` feature provides the client every server runs:

```rust
let keys = KeyDirectory::new(&args.key_directory);
let key = keys.load_or_generate()?;
let endpoint = RegistrationClient::discover(&http, &hub_issuer).await?;
let metadata = ClientMetadata::new(
    *key.public_key(),
    registration,
    <TimeDomain as DomainServer>::routes().authorization(),
)?;
let client = RegistrationClient::new(http, endpoint, registration_secret, key, metadata)?;
let registration = client.resume_or_register(&keys).await?;
let database = registration.database();
let server = HostedServer::for_domain::<TimeDomain>()
    .deployment(&deployment, false)?
    .internal_trust(registration.access_tokens(), http.clone())?;
tokio::spawn(async move { client.maintain(registration, shutdown_signal()).await });
```

`KeyDirectory` generates the Ed25519 key on first start and stores it as PKCS#8 in
`server-key.p8`, readable only by its owner and written atomically. Restarts reuse the
key. `KeyDirectory::prepare(declaration, authorization)` emits exact `ClientMetadata`
with that persisted key entirely offline: no Hub token, application database or
registration is needed to describe a candidate before trusted scoped admission.

Each immutable deployment owns its own `KeyDirectory`. A new deployment receives a
new worker key/state directory; it never copies the namespace owner's private key or
another deployment's registration. The trusted runtime selects the directory.

`resume_or_register` stores the standard private `ClientInformation` in
`registration.json` beside the key, with mode 0600 and atomic replacement. An existing
record must match the worker key and full immutable declaration, including deployment,
slug and endpoint. Its configuration URI must belong to the configured Hub authority.
RFC 7592 GET must return the same client ID and configuration URI; only for that same
verified registration is its original private database grant retained when GET omits
the password. The restored grant remains in the native `ClientInformation` object.

An absent, unreadable or non-matching local record, or a registration the Hub
answers with 401 or 404, registers again with the initial access token and replaces
the saved state. A changed declaration, a lost registration or a new Hub authority
therefore never stops the server: the token is the only check, and the Hub decides
what a registration with it replaces. Remote registration and the local write are
not one transaction: after a lost response or a failed write, the next start
registers again.

`maintain` waits for process shutdown without deleting registration state. Explicit
`deregister` retires the registration; the operator also removes its private state
when retiring the deployment. The Hub's ordinary orphan grace may collect a stopped
registration, after which restart needs a fresh scoped admission. `update` is still
RFC 7592; immutable deployment metadata changes require a new deployment. Callers
must store any explicitly rotated credentials before their next restart. The caller
supplies `reqwest::Client` and installs a rustls crypto provider before HTTPS.

## Crate Features

| Feature of `veoveo-hub-sdk` | Adds to this module |
|---|---|
| `contract` | Declarations, the OAuth request and response types, the token exchange wire types, `PermissionsDetail`, JWK types and their schemas |
| `server` | `ServerKeyPair`, the software statement and client assertion on AWS-LC, `KeyDirectory` and `RegistrationClient` on reqwest, and advertising and reading the declarations in MCP capabilities |

## Qualification

`tests/wire.rs` fixes the wire form of every declaration, request, response, error and
authorization detail, and validates the examples against the generated JSON Schemas.
It checks the RFC 7638 thumbprint against RFC 8037 Appendix A. `tests/client.rs` runs
the registration client against an in-process endpoint that applies the admission
steps above: registration, read, update with token rotation, an update of a removed
registration, removal at shutdown, the pinned key and the initial access token. The software statement
tests cover the pinned key, metadata, audience and lifetime. The forwarded token
tests in `veoveo-hub-sdk` cover the token type, issuer, audience, actor, `jti`,
the single authorization detail, the namespace of granted permissions and agreement
between standard and Veoveo claims. Its key refresh tests run an issuer on a loopback
port: a rotated key, one fetch for concurrent requests, the 30-second interval, a set
without the token's key, a foreign issuer, a missing or remote plain-HTTP `jwks_uri`, a
redirect, an oversized document and an unreachable issuer.


## Durable Deployment Affinity

The Core Task runtime has a required immutable `DeploymentId`; creation attaches it,
and reads, claims, recovery and idempotency remain in that deployment even when two
logical service instances deliberately share a schema. SDK Task projections and
`ServerCalls::for_task` reject snapshots from another deployment. Adopted component
entrypoints must construct both runtime and call adapter from the same admitted
registration, and use `resume_or_register` uniformly when repinning this contract.

`ActingServer` and `DelegatingTask` retain the originating deployment. Signed nested
Task constraints compare both logical slug and deployment; the per-hop target may
change, while inherited `ExecutionContext` stays unchanged. Callback URLs and external
provider ingress still require deployment-specific qualification by their owning
adapter; queue affinity alone does not establish multi-deployment callback routing.
