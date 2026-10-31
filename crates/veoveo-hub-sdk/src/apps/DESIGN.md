# MCP Apps Contract Design

This document is the contract for how servers ship user-facing views to the Hub, the
installation's MCP Apps host, and to any other MCP Apps host. The `apps` module of
`veoveo-hub-sdk` owns the pinned protocol constants and helpers; this document owns the
rules.

## Standards And Protocols

| Standard or protocol | Implemented profile |
|---|---|
| [Model Context Protocol](https://modelcontextprotocol.io/specification/) | App discovery and invocation remain ordinary MCP resource, tool and Task traffic, version `2026-07-28`, through the Hub. |
| MCP Apps SEP-1865 / `ext-apps` | Version `2026-01-26`: `ui://` resources, `text/html;profile=mcp-app`, tool-to-app metadata, host context and display modes, lifecycle notifications, the `postMessage` bridge, and for a web host the sandbox proxy on an origin different from the host's. Where the specification provides a mechanism, the contract uses it and adds no metadata. |
| Veoveo reactive App resource adapter | Repository-owned extension over MCP Apps `2026-01-26`, which has no final resource listener. An App requests an authorized resource filter through one `subscriptions/listen` stream; the host projects contentless `ui/notifications/resource-updated` wakes from its auth-scoped MCP client. This adapter is not part of SEP-1865. |
| Veoveo internal App navigation adapter | Repository-owned `ui/open-link` profile. An exact `ui://` target navigates only when it is in the caller-visible App catalog. Other targets fail closed. |
| MCP Tasks extension `io.modelcontextprotocol/tasks` | Version `2026-07-28`. App-started durable work keeps the lifecycle and ownership rules of an ordinary MCP client. |
| Owned Apps metadata | The host and server serialize the same `AppExtensionCapability` MIME declaration. `ToolUiMeta`, `ResourceUiMeta` and their nested types expose owner-generated schemas for the adopted upstream profile. |
| [JSON Schema Draft 2020-12](https://json-schema.org/draft/2020-12/) | Linked tool arguments and structured results use the same canonical schemas exposed outside the app. |
| HTML iframe sandbox and Content Security Policy | An App runs in an inner `sandbox="allow-scripts"` frame, without `allow-same-origin`, so it has an opaque origin, no cookies and no storage. The inner frame sits in the host's sandbox proxy, which is served from its own origin. The host builds each App's CSP from its validated `_meta.ui.csp` declarations. |

## The rule

A server's operational surface crosses one protocol boundary: MCP.

- **Reads are resources.** Domain state an operator or app needs is a `{slug}://…`
  resource, with templates for addressable entities.
- **Changes are tools.** Administrative writes are ordinary MCP tools, scope-gated inside
  the server and policy-gated at the Hub.
- **Views are app resources.** Interactive UI ships as an HTML document at
  `ui://{slug}/{page}` with MIME `text/html;profile=mcp-app`.

Servers do not expose bespoke administrative REST routers, and hosts do not hardcode a
server's pages, navigation entries or proxy routes. If a server needs UI, it ships an
app; if it needs new operations, it ships tools.

One exception exists. A server whose UI is an existing full web application, which
needs its own cookies, storage and HTTP routes, serves it through its
`authenticated_routes` on its own app origin. Its app resource frames that origin. The
app resource stays the entry point that hosts discover and authorize.

## Server Obligations

A server shipping a view (Time's app is the reference implementation) declares it in its
typed `Routes`, from which discovery, reading, registration requirements and capabilities
derive, as for its other routes:

1. Declare the view: `RoutesBuilder::app` takes an `AppBuilder` with the App's
   `ui://{slug}/{page}` address as an `AppUri`, which must be under the server's own slug,
   its name, title, description, icons as `data:` URIs of images and UI metadata, its
   `RoutePolicy`, and a reader that returns the server's `AppHtml`. Discovery lists it with
   the App MIME type, a read returns the document as text under that MIME type, and the
   registration's requirements cover its URI. A client may reuse a read for the private
   resource lifetime, as the document is an immutable startup snapshot. Hosts build
   catalog and navigation entries from these fields.
2. The document is self-contained unless its function requires a declared live-data
   connection. Such a view declares exact installation-owned origins in its UI metadata's
   `csp`; it names no wildcards, paths, credentials, queries or fragments.
3. Link tools: `RoutesBuilder::app_tools` names the App's URI, the server's tools the view
   may invoke and their visibility, such as model and app. Each linked tool carries
   `_meta.ui` naming the App in `tools/list` and in tool lookup. `_meta.ui.resourceUri`
   names one App, so a tool links to one App. Building the routes fails for an unknown
   tool or App, or a tool linked twice. A tool without an app link is never app-callable.
4. Capability: when the routes include an App, the setup declares the
   `io.modelcontextprotocol/ui` extension with the App MIME type, and only then.
5. Gate in the server: administrative tools check the domain's administrative scope as
   any scoped tool does; resources carry the scope their data warrants.
6. Keep JSON numbers within the range a JavaScript number holds exactly, as I-JSON
   (RFC 7493) recommends, in every result an App may read: views run in browsers.
7. Choose presentation through the specification: the view declares the display modes it
   supports in `ui/initialize` and asks for a change with `ui/request-display-mode`. A
   card supports `inline`; an editor supports only `fullscreen`.
8. A full web application: the view declares its server's app origin, and only that
   origin, in `_meta.ui.csp.frameDomains` and frames it.

The lower-level helpers (`extend_capabilities`, `app_resource`, `app_html_contents`,
`link_tool_to_app`) remain for a server that does not host through `Routes`.

### Packaged App Assets

`AppHtml` reads an image-owned HTML file into an immutable startup snapshot. It requires
a regular, nonempty UTF-8 file within the host's 2 MiB limit. Missing or invalid assets
fail startup. The server applies the same resource authorization to these bytes as to an
embedded document; the loader creates no route or permission.

A server packages its App HTML in its own image and passes the file path at startup. The
loader reads the file on each process start, so new bytes arrive with a new image.

## Host Obligations

The Hub is the installation's host. Its design owns the routes; this contract fixes what
any host does.

- **Direct URL.** `/apps/{slug}/{page}` on the Hub's public host opens
  `ui://{slug}/{page}` for the signed-in person, against the branch the person selected
  for that server. The host parses the path into one canonical URI with a typed slug and
  renders the App only when that exact resource is in the caller-visible catalog. Browser
  code never derives App authority from the path. A direct URL renders the App with the
  display mode `fullscreen`.
- **Sandbox proxy.** The host page and the sandbox proxy have different origins. The proxy
  is the host's own code, the same for every App, and holds no session or data. It creates
  each App's inner frame with `sandbox="allow-scripts"` and never adds
  `allow-same-origin`, and it forwards messages between the host and the App. The host
  accepts a message only from the proxy frame it created for that App, so one App cannot
  speak as another.
- **CSP.** The host validates every declared origin and adds only those exact sources to
  their directives. `frameDomains` admits only the App's own server's app origin. Local
  `data:` fetches add no remote origin. A self-contained view's inline script and style
  run inside its opaque frame; the frame stays `sandbox="allow-scripts"` alone, so forms
  do not submit and views act on buttons and keys.
- **Host context.** The host supplies the theme, `timeZone` and `availableDisplayModes`;
  a direct URL offers at least `fullscreen`.
- **Catalog.** The host discovers Apps from `resources/list` (`is_app_resource`), derives
  ownership from the `ui://{server}/…` prefix and attaches only that server's app-visible
  linked tools. Nothing is registered by hand. A failed server's discovery is isolated;
  healthy Apps stay available beside a degradation notice.
- **Bridge.** The host declares `serverTools` and `serverResources`. It proxies a view's
  `tools/call` only to app-visible tools linked to that exact view on that view's server,
  through the shared admission below. A view names a tool by its server's own name, such
  as `resolve_time`, and the host maps it to the catalog name `{slug}__{tool}`. A view
  reads its own server's resources, at their own addresses including query forms, and no
  other server's. Every call runs through the Hub's ordinary authorization as the
  signed-in person; browser code holds no bearer token. The host passes structured
  results through without changing their numbers.
- **Tasks.** Task-based tools stay task-based inside Apps. A view may send `tools/call`
  with final request metadata and `tasks/get`, `tasks/update` and `tasks/cancel`; the
  host's client declares the Tasks client capability for App traffic, records Task
  ownership per view and forwards Task requests as the person, passing `resultType: "task"`
  results and `tasks/get` results and errors through unchanged. Only app-visible linked
  tools may start Tasks, and a view may poll only Tasks it started.
- **Reactive resources.** The host serves the reactive adapter: it registers a view's
  bounded subscription set on its auth-scoped MCP client and delivers
  `ui/notifications/resource-updated` wakes into the frame. On the wire, the view sends
  `subscriptions/listen` with `{"notifications":{"resourceSubscriptions":[uri, …]}}`;
  the host answers `notifications/subscriptions/acknowledged`; each wake carries
  `{"uri": …}`; and the view ends the listen with `notifications/cancelled` naming its
  `requestId`. Domain state never travels in
  a wake; the view reads current state through an ordinary bridge request. Configured
  ceilings bound listeners per browser session, and authorization expiry closes them.
- **Navigation.** An App may send `ui/open-link` for another exact `ui://` resource; the
  host resolves it against the caller-visible catalog and opens that App's direct URL.
- **App origins.** A server's app origin, `{slug}.apps.{host}`, carries only that server's
  `authenticated_routes`, behind the permission the server declares for it. The origin is
  the server's alone: it is never the host page's origin, the proxy's origin or another
  server's.

## View Obligations

- `ui/initialize`: apply `hostContext`, its theme and display mode, then send
  `ui/notifications/initialized`.
- A self-driving view loads its own data through `resources/read` after initialize and
  renders meaningful content without waiting for a `tool-result` push.
- Report height through `ui/notifications/size-changed`; answer `ui/resource-teardown`.
- Handle tool and resource failures inline: authorization errors are ordinary failed
  requests, after which the view degrades to read-only or shows the error.
- A reactive view subscribes only to resources its own server owns, treats each update as
  a wake to read current state, and unsubscribes during teardown. It uses no timer in place
  of resource updates.

## Shared Host Admission

`admission.rs` resolves an App-local tool alias from the current caller-visible resource
and tool catalogs. A linked tool declares App visibility for that exact view, on the view's
own server. Untrusted frame metadata never enters this resolver.

## Why Not Administrative REST

A server-local administrative REST router, a Hub proxy for it and a hand-written host page
per server would need four hardcoded integration points per server. Apps replace all four
with discovery. A full web application on its own app origin is the one exception, and its
app resource still enters through discovery.
