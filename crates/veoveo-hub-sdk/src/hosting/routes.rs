//! Typed server registration. Raw MCP results are constructed only after a
//! registered request/output association and its current policy have succeeded.
use super::{DomainServer, ReadCache, hub_identity};
use crate::{
    apps::{AppHtml, AppUri, ResourceUiMeta, ToolUiMeta, UiVisibility},
    calls::{ResourceRead, ToolCall},
    definition::{
        CollectionRequest, CollectionResource, CollectionTool, List, ReadCollection, SingleOutput,
    },
    registration::{
        BodyLimit, PermissionDeclaration, PermissionDefinition, PublicRouteDeclaration,
        PublicRoutePrefix, RequestRate, SurfacesDeclaration,
    },
    server_contract::{McpResource, McpResourceTemplate, McpServerContract},
};
use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult, Icon,
        MetaObject, Prompt, ReadResourceResult, Resource, ResourceTemplate, Tool,
    },
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
};
use veoveo_types::{
    Collection, Page, PermissionName, ResourceAddress, ResourceTemplateUri, ResourceTypeName,
};

type Pending<T> = Pin<Box<dyn Future<Output = Result<T, ErrorData>> + Send>>;
type ToolHandler<D> = Arc<
    dyn Fn(D, CallToolRequestParams, RequestContext<RoleServer>) -> Pending<CallToolResult>
        + Send
        + Sync,
>;
type PromptHandler<D> = Arc<
    dyn Fn(D, GetPromptRequestParams, RequestContext<RoleServer>) -> Pending<GetPromptResult>
        + Send
        + Sync,
>;
type ReadHandler<D> = Arc<
    dyn Fn(
            D,
            veoveo_types::ResourceUri,
            RequestContext<RoleServer>,
        ) -> Option<Pending<(ReadResourceResult, ReadCache)>>
        + Send
        + Sync,
>;

#[derive(Clone, Debug)]
pub enum RoutePolicy {
    Authenticated,
    Permission(PermissionDeclaration),
}

/// Typed tool rejection; only the transport adapter materializes MCP error data.
#[derive(Debug)]
pub enum ToolError {
    Protocol(ErrorData),
    Rejected(String),
}
impl From<ErrorData> for ToolError {
    fn from(error: ErrorData) -> Self {
        Self::Protocol(error)
    }
}
impl From<crate::client::CallError> for ToolError {
    fn from(error: crate::client::CallError) -> Self {
        Self::Rejected(error.to_string())
    }
}
impl RoutePolicy {
    pub fn access(&self) -> crate::registration::RouteAccess {
        match self {
            Self::Authenticated => crate::registration::RouteAccess::Authenticated,
            Self::Permission(permission) => {
                crate::registration::RouteAccess::Permission(permission.permission.clone())
            }
        }
    }
    pub fn permission<P: PermissionDefinition + Copy>(
        permission: P,
    ) -> Result<Self, crate::registration::DeclarationError> {
        Ok(Self::Permission(PermissionDeclaration {
            permission: PermissionName::parse(permission.as_str()).map_err(|_| {
                crate::registration::DeclarationError::invalid(
                    "permission",
                    "invalid route permission",
                )
            })?,
            resource: ResourceTypeName::parse(permission.resource()).map_err(|_| {
                crate::registration::DeclarationError::invalid(
                    "resource",
                    "invalid route resource type",
                )
            })?,
            access: permission.access(),
            risks: permission.risks().iter().copied().collect(),
        }))
    }
    pub fn check(&self, context: &RequestContext<RoleServer>) -> Result<(), ErrorData> {
        self.check_identity(&hub_identity(context)?)
    }
    pub(crate) fn check_identity(
        &self,
        identity: &crate::HubInternalIdentity,
    ) -> Result<(), ErrorData> {
        if let Self::Permission(permission) = self
            && !identity.permissions.contains(&permission.permission)
        {
            return Err(ErrorData::invalid_request(
                format!(
                    "required route permission `{}` was not granted",
                    permission.permission
                ),
                None,
            ));
        }
        Ok(())
    }
}

fn annotations(policy: &RoutePolicy) -> Option<rmcp::model::ToolAnnotations> {
    let RoutePolicy::Permission(permission) = policy else {
        return None;
    };
    Some(
        rmcp::model::ToolAnnotations::new()
            .read_only(permission.access == crate::registration::Access::Read)
            .destructive(
                permission
                    .risks
                    .contains(&crate::registration::Risk::Destructive),
            ),
    )
}

pub struct Missing;
pub struct Ready<T>(T);

/// Required resource construction steps: typed reader, current access policy and
/// cache policy. A partial builder cannot be registered or served.
pub struct ResourceBuilder<R, H = Missing, P = Missing, K = Missing> {
    name: &'static str,
    reader: H,
    policy: P,
    cache: K,
    read: PhantomData<fn() -> R>,
}
impl<R: ResourceRead> ResourceBuilder<R> {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            reader: Missing,
            policy: Missing,
            cache: Missing,
            read: PhantomData,
        }
    }
}
impl<R, H, P, K> ResourceBuilder<R, H, P, K> {
    pub fn reader<F>(self, reader: F) -> ResourceBuilder<R, Ready<F>, P, K> {
        ResourceBuilder {
            name: self.name,
            reader: Ready(reader),
            policy: self.policy,
            cache: self.cache,
            read: PhantomData,
        }
    }
    pub fn policy(self, policy: RoutePolicy) -> ResourceBuilder<R, H, Ready<RoutePolicy>, K> {
        ResourceBuilder {
            name: self.name,
            reader: self.reader,
            policy: Ready(policy),
            cache: self.cache,
            read: PhantomData,
        }
    }
    pub fn cache(self, cache: ReadCache) -> ResourceBuilder<R, H, P, Ready<ReadCache>> {
        ResourceBuilder {
            name: self.name,
            reader: self.reader,
            policy: self.policy,
            cache: Ready(cache),
            read: PhantomData,
        }
    }
}

/// One MCP App the routes declare: its `ui://{slug}/{page}` address, the fields hosts
/// build catalog and navigation entries from, its access policy and a reader that
/// returns the server's [`AppHtml`]. The reader and the policy are required steps,
/// so a partial App cannot be registered.
pub struct AppBuilder<H = Missing, P = Missing> {
    uri: AppUri,
    name: String,
    title: Option<String>,
    description: Option<String>,
    icons: Vec<Icon>,
    ui: ResourceUiMeta,
    reader: H,
    policy: P,
}
impl AppBuilder {
    /// An App at `uri` whose programmatic resource name is `name`.
    pub fn new(uri: AppUri, name: impl Into<String>) -> Self {
        Self {
            uri,
            name: name.into(),
            title: None,
            description: None,
            icons: Vec::new(),
            ui: ResourceUiMeta::default(),
            reader: Missing,
            policy: Missing,
        }
    }
}
impl<H, P> AppBuilder<H, P> {
    /// The title hosts show for the App.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// What the App is for.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
    /// An icon, whose `src` is an RFC 2397 `data:` URI of an image.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }
    /// The App's `_meta.ui`: its Content Security Policy declarations and framing.
    pub fn ui(mut self, ui: ResourceUiMeta) -> Self {
        self.ui = ui;
        self
    }
    pub fn reader<F>(self, reader: F) -> AppBuilder<Ready<F>, P> {
        AppBuilder {
            uri: self.uri,
            name: self.name,
            title: self.title,
            description: self.description,
            icons: self.icons,
            ui: self.ui,
            reader: Ready(reader),
            policy: self.policy,
        }
    }
    pub fn policy(self, policy: RoutePolicy) -> AppBuilder<H, Ready<RoutePolicy>> {
        AppBuilder {
            uri: self.uri,
            name: self.name,
            title: self.title,
            description: self.description,
            icons: self.icons,
            ui: self.ui,
            reader: self.reader,
            policy: Ready(policy),
        }
    }
}

/// Whether `src` is an RFC 2397 `data:` URI of an image.
fn is_image_data_uri(src: &str) -> bool {
    src.strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .is_some_and(|(media_type, _)| media_type.starts_with("image/"))
}

/// The tools one App may invoke, as `RoutesBuilder::app_tools` named them.
struct AppLink {
    app: AppUri,
    tools: Vec<String>,
    visibility: Vec<UiVisibility>,
}

use crate::definition::TemplateRead;

struct ToolRoute<D> {
    descriptor: Tool,
    policy: RoutePolicy,
    invoke: ToolHandler<D>,
    task: bool,
}
struct PromptRoute<D> {
    descriptor: Prompt,
    policy: RoutePolicy,
    invoke: PromptHandler<D>,
}
struct ResourceRoute<D: DomainServer> {
    fixed: Option<McpResource>,
    template: Option<McpResourceTemplate>,
    policy: RoutePolicy,
    invoke: ReadHandler<D>,
}

pub struct Routes<D: DomainServer> {
    inner: RoutesBuilder<D>,
}
impl<D: DomainServer> Routes<D> {
    pub(crate) fn has_prompts(&self) -> bool {
        !self.inner.prompts.is_empty()
    }
    pub(crate) fn list_prompts(&self, context: &RequestContext<RoleServer>) -> Vec<Prompt> {
        self.inner
            .prompts
            .values()
            .filter(|route| route.policy.check(context).is_ok())
            .map(|route| route.descriptor.clone())
            .collect()
    }
    pub(crate) async fn prompt(
        &self,
        domain: D,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        let route = self
            .inner
            .prompts
            .get(&request.name)
            .ok_or_else(|| super::unknown_prompt(&request.name))?;
        route.policy.check(&context)?;
        (route.invoke)(domain, request, context).await
    }
    pub(crate) fn has_tasks(&self) -> bool {
        self.inner.tools.values().any(|route| route.task)
    }
    pub(crate) fn has_tools(&self) -> bool {
        !self.inner.tools.is_empty()
    }
    pub(crate) fn has_apps(&self) -> bool {
        !self.inner.apps.is_empty()
    }
    /// The descriptions the server changes while it runs, when it has them.
    pub(crate) fn descriptions(&self) -> Option<&super::ToolDescriptions> {
        self.inner.descriptions.as_ref()
    }
    pub fn builder(documents: RoutePolicy) -> RoutesBuilder<D> {
        RoutesBuilder::new(documents)
    }
    pub fn authorization(&self) -> crate::registration::AuthorizationDeclaration {
        self.inner.authorization()
    }
    /// The HTTP the routes serve besides MCP: the `surfaces` of the server's
    /// [`RegistrationDeclaration`](crate::registration::RegistrationDeclaration).
    pub fn surfaces(&self) -> Option<SurfacesDeclaration> {
        self.inner.surfaces()
    }
    /// The declared public route prefixes relative to the mount, such as `/hooks/`.
    pub(crate) fn public_prefixes(&self) -> Vec<String> {
        let slug = crate::server_contract::provider_slug::<D::Contract>();
        self.inner
            .public_routes
            .iter()
            .filter_map(|route| route.prefix.within_mount(&slug).map(str::to_owned))
            .collect()
    }

    pub(crate) fn resources(&self) -> Vec<McpResource> {
        self.inner.resources()
    }
    pub(crate) fn templates(&self) -> Vec<McpResourceTemplate> {
        self.inner.templates()
    }
    pub(crate) fn list_tools(&self, context: &RequestContext<RoleServer>) -> Vec<Tool> {
        self.inner.list_tools(context)
    }
    pub(crate) fn get_tool(&self, name: &str) -> Option<Tool> {
        self.inner.get_tool(name)
    }
    pub(crate) fn authorize_tool(
        &self,
        name: &str,
        context: &RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.inner
            .tools
            .get(name)
            .ok_or_else(|| ErrorData::invalid_params("unknown typed tool", None))?
            .policy
            .check(context)
    }
    pub(crate) fn resource_visible(&self, uri: &str, context: &RequestContext<RoleServer>) -> bool {
        for route in &self.inner.resources {
            if route
                .fixed
                .as_ref()
                .is_some_and(|resource| resource.descriptor().uri == uri)
            {
                return route.policy.check(context).is_ok();
            }
        }
        let Ok(parts) = veoveo_types::ResourceUriParts::parse(uri) else {
            return false;
        };
        parts.scheme() == <D::Contract as McpServerContract>::scheme().as_str()
            && matches!(parts.authority(), "docs" | "contract")
            && self.inner.documents.check(context).is_ok()
    }
    pub(crate) fn template_visible(
        &self,
        template: &str,
        context: &RequestContext<RoleServer>,
    ) -> bool {
        for route in &self.inner.resources {
            if route
                .template
                .as_ref()
                .is_some_and(|resource| resource.template().as_str() == template)
            {
                return route.policy.check(context).is_ok();
            }
        }
        template
            == crate::docs::docs_member_template(&<D::Contract as McpServerContract>::scheme())
                .as_str()
            && self.inner.documents.check(context).is_ok()
    }
    pub(crate) fn authorize_documents(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.inner.documents.check(context)
    }
    pub(crate) fn authorize_documents_identity(
        &self,
        identity: &crate::HubInternalIdentity,
    ) -> Result<(), ErrorData> {
        self.inner.documents.check_identity(identity)
    }
    pub(crate) async fn read(
        &self,
        domain: D,
        address: veoveo_types::ResourceUri,
        context: RequestContext<RoleServer>,
    ) -> Result<(ReadResourceResult, ReadCache), ErrorData> {
        self.inner.read(domain, address, context).await
    }
    pub(crate) async fn call(
        &self,
        domain: D,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.inner.call(domain, request, context).await
    }
}
pub struct RoutesBuilder<D: DomainServer> {
    tools: BTreeMap<String, ToolRoute<D>>,
    prompts: BTreeMap<String, PromptRoute<D>>,
    resources: Vec<ResourceRoute<D>>,
    documents: RoutePolicy,
    uses: Vec<PermissionName>,
    calls_hub: bool,
    descriptions: Option<super::ToolDescriptions>,
    apps: BTreeSet<AppUri>,
    app_links: Vec<AppLink>,
    public_routes: Vec<PublicRouteDeclaration>,
    app_origin: Option<RoutePolicy>,
}
impl<D: DomainServer> RoutesBuilder<D> {
    fn new(documents: RoutePolicy) -> Self {
        Self {
            tools: BTreeMap::new(),
            prompts: BTreeMap::new(),
            resources: vec![],
            documents,
            uses: vec![],
            calls_hub: false,
            descriptions: None,
            apps: BTreeSet::new(),
            app_links: vec![],
            public_routes: vec![],
            app_origin: None,
        }
    }
    pub fn build(mut self) -> Routes<D> {
        let _ = self.permission_declarations();
        self.link_app_tools();
        if let Some(surfaces) = self.surfaces() {
            let slug = crate::server_contract::provider_slug::<D::Contract>();
            if let Err(error) = surfaces.check(&slug, self.app_origin.is_some()) {
                panic!("invalid typed route surfaces: {error}");
            }
        }
        Routes { inner: self }
    }
    /// Attach each App link to its tool's descriptor as `_meta.ui`. Every link names a
    /// declared App and declared tools, and a tool links to one App.
    fn link_app_tools(&mut self) {
        let mut linked = BTreeSet::new();
        for link in std::mem::take(&mut self.app_links) {
            assert!(
                self.apps.contains(&link.app),
                "App tool links name an App the routes do not declare"
            );
            let meta = serde_json::to_value(ToolUiMeta {
                resource_uri: link.app.as_str().to_owned(),
                visibility: (!link.visibility.is_empty()).then_some(link.visibility),
            })
            .expect("tool UI metadata serializes");
            for tool in link.tools {
                let route = self
                    .tools
                    .get_mut(&tool)
                    .expect("App tool links name a tool the routes do not declare");
                assert!(linked.insert(tool), "a tool links to one App");
                route
                    .descriptor
                    .meta
                    .get_or_insert_with(MetaObject::new)
                    .insert(crate::apps::UI_META_KEY.to_owned(), meta.clone());
            }
        }
    }
    fn permission_declarations(&self) -> BTreeMap<PermissionName, PermissionDeclaration> {
        let policies = std::iter::once(&self.documents)
            .chain(self.tools.values().map(|route| &route.policy))
            .chain(self.prompts.values().map(|route| &route.policy))
            .chain(self.resources.iter().map(|route| &route.policy))
            .chain(self.app_origin.iter());
        let mut permissions = BTreeMap::new();
        for policy in policies {
            if let RoutePolicy::Permission(permission) = policy
                && let Some(previous) =
                    permissions.insert(permission.permission.clone(), permission.clone())
            {
                assert_eq!(
                    previous, *permission,
                    "conflicting typed route permission declaration"
                );
            }
        }
        permissions
    }
    fn insert_resource(&mut self, route: ResourceRoute<D>) {
        let reserved = |uri: &veoveo_types::ResourceUri| {
            let parts = uri.components().expect("typed resource components");
            parts.scheme() == <D::Contract as McpServerContract>::scheme().as_str()
                && matches!(parts.authority(), "docs" | "contract")
        };
        if let Some(fixed) = &route.fixed {
            assert!(
                !reserved(fixed.address()),
                "typed resource route collides with host-owned documents"
            );
            assert!(
                self.resources.iter().all(|existing| existing
                    .fixed
                    .as_ref()
                    .is_none_or(|other| other.address() != fixed.address())),
                "duplicate typed fixed resource route"
            );
        }
        if let Some(template) = &route.template {
            let parsed = crate::registration::authorization_template::AuthorizationTemplate::parse(
                template.template(),
            )
            .expect("native typed resource template");
            assert!(
                !(parsed.scheme() == <D::Contract as McpServerContract>::scheme().as_str()
                    && matches!(parsed.authority(), "docs" | "contract")),
                "typed resource template collides with host-owned documents"
            );
            assert!(
                self.resources.iter().all(|existing| existing
                    .template
                    .as_ref()
                    .is_none_or(|other| other.template() != template.template())),
                "duplicate typed resource template route"
            );
        }
        self.resources.push(route);
    }
    pub fn uses(mut self, permission: PermissionName) -> Self {
        self.uses.push(permission);
        self
    }
    /// Declare that the handlers call the Hub's own routes for their caller, with the
    /// bearer [`HubClient::hub_bearer`](crate::client::HubClient::hub_bearer) returns.
    /// The server then registers the token exchange grant.
    pub fn calls_hub(mut self) -> Self {
        self.calls_hub = true;
        self
    }
    /// Declare a public route prefix: `path` under the mount, ending with `/`, such as
    /// `hooks/`, with the largest request body it admits and its request rate. Public
    /// routes carry no Hub authentication, so they serve only callers that verify
    /// themselves. The registration and Discover declare the prefix, and the hosted
    /// server serves no public route outside its declared prefixes. Building the routes
    /// fails for a prefix the [surfaces](crate::registration::SurfacesDeclaration::check)
    /// rules refuse.
    pub fn public_route(
        mut self,
        path: &str,
        max_body_bytes: u64,
        requests_per_minute: u32,
    ) -> Self {
        let slug = crate::server_contract::provider_slug::<D::Contract>();
        self.public_routes.push(PublicRouteDeclaration {
            prefix: PublicRoutePrefix::under(&slug, path).expect("valid public route prefix"),
            max_body_bytes: BodyLimit::new(max_body_bytes).expect("valid public route body limit"),
            requests_per_minute: RequestRate::per_minute(requests_per_minute)
                .expect("valid public route rate"),
        });
        self
    }
    /// Declare the SDK's operation callbacks, `{mount}/operations/`, which
    /// [`operation_callbacks`](super::operations::operation_callbacks) serves, with its
    /// body limit and [`EVENT_REQUESTS_PER_MINUTE`](super::operations::EVENT_REQUESTS_PER_MINUTE).
    pub fn operation_callbacks(self) -> Self {
        self.public_route(
            super::operations::OPERATIONS_PATH,
            super::operations::MAX_EVENT_BYTES as u64,
            super::operations::EVENT_REQUESTS_PER_MINUTE,
        )
    }
    /// Declare the SDK's provider events, `{mount}/providers/`, which
    /// [`provider_events`](super::operations::provider_events) serves, with its body
    /// limit and [`EVENT_REQUESTS_PER_MINUTE`](super::operations::EVENT_REQUESTS_PER_MINUTE).
    pub fn provider_events(self) -> Self {
        self.public_route(
            super::operations::PROVIDERS_PATH,
            super::operations::MAX_EVENT_BYTES as u64,
            super::operations::EVENT_REQUESTS_PER_MINUTE,
        )
    }
    /// Declare that the server serves its `authenticated_routes` on its own app origin,
    /// for a full web application, and admits a person there by `policy`, the `app_origin`
    /// requirement of its authorization declaration.
    pub fn app_origin(mut self, policy: RoutePolicy) -> Self {
        self.app_origin = Some(policy);
        self
    }
    /// The HTTP the routes serve besides MCP, as the registration and Discover declare
    /// it; `None` when they serve none.
    pub fn surfaces(&self) -> Option<SurfacesDeclaration> {
        let surfaces = SurfacesDeclaration {
            public_routes: self.public_routes.clone(),
            app_origin: self.app_origin.is_some(),
        };
        (!surfaces.is_empty()).then_some(surfaces)
    }
    /// Serve each tool's description from `descriptions` once the server sets one,
    /// and the one its route registered until then. The server then advertises
    /// `tools.listChanged` and tells its listeners after every change.
    pub fn descriptions(mut self, descriptions: super::ToolDescriptions) -> Self {
        self.descriptions = Some(descriptions);
        self
    }
    /// A tool's descriptor with its current description.
    fn current(&self, descriptor: &Tool) -> Tool {
        let mut descriptor = descriptor.clone();
        if let Some(description) = self
            .descriptions
            .as_ref()
            .and_then(|descriptions| descriptions.get(&descriptor.name))
        {
            descriptor.description = Some(description.to_string().into());
        }
        descriptor
    }
    /// Register one native prompt descriptor and its rendering operation with the
    /// same explicit policy used by discovery and registration requirements.
    pub fn prompt<H, F>(mut self, descriptor: Prompt, policy: RoutePolicy, handler: H) -> Self
    where
        H: Fn(D, GetPromptRequestParams, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<GetPromptResult, ErrorData>> + Send + 'static,
    {
        let name =
            crate::identity::PromptName::parse(&descriptor.name).expect("valid typed prompt name");
        let invoke: PromptHandler<D> =
            Arc::new(move |domain, request, context| Box::pin(handler(domain, request, context)));
        assert!(
            self.prompts
                .insert(
                    name.as_str().to_owned(),
                    PromptRoute {
                        descriptor,
                        policy,
                        invoke
                    }
                )
                .is_none(),
            "duplicate typed prompt route"
        );
        self
    }
    /// Register one MCP App. Its address lies under the server's own `ui://{slug}/`.
    /// Discovery lists it with the App MIME type and its UI metadata, a read returns
    /// the reader's document as text under that MIME type, and the registration's
    /// requirements cover its address with `policy`. A server whose routes hold an App
    /// declares the `io.modelcontextprotocol/ui` extension. A client may reuse a read
    /// for [`PRIVATE_RESOURCE_TTL_MS`](crate::PRIVATE_RESOURCE_TTL_MS), since the
    /// document is an immutable startup snapshot.
    pub fn app<H, F>(mut self, app: AppBuilder<Ready<H>, Ready<RoutePolicy>>) -> Self
    where
        H: Fn(D, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<AppHtml, ErrorData>> + Send + 'static,
    {
        assert!(
            *app.uri.server() == crate::server_contract::provider_slug::<D::Contract>(),
            "App address lies outside the server's ui://{{slug}}/"
        );
        assert!(
            app.icons.iter().all(|icon| is_image_data_uri(&icon.src)),
            "App icons are data: URIs of images"
        );
        let mut descriptor =
            crate::apps::app_resource_with_meta(app.uri.as_str(), &app.name, app.ui.clone());
        descriptor.title = app.title;
        descriptor.description = app.description;
        descriptor.icons = (!app.icons.is_empty()).then_some(app.icons);
        let uri = app.uri.clone();
        let fixed = McpResource::new(uri.clone(), |_| descriptor).expect("typed App descriptor");
        let policy = app.policy.0;
        let access = policy.clone();
        let handler = Arc::new(app.reader.0);
        let invoke: ReadHandler<D> = Arc::new(move |domain, requested, context| {
            if requested != *uri.resource_uri() {
                return None;
            }
            let access = access.clone();
            let handler = handler.clone();
            let uri = uri.clone();
            Some(Box::pin(async move {
                access.check(&context)?;
                let html = handler(domain, context).await?;
                let contents = crate::apps::app_html_contents(uri.as_str(), html.as_str());
                Ok((ReadResourceResult::new(vec![contents]), ReadCache::Private))
            }))
        });
        self.apps.insert(app.uri);
        self.insert_resource(ResourceRoute {
            fixed: Some(fixed),
            template: None,
            policy,
            invoke,
        });
        self
    }
    /// Link the server's `tools` to the App at `app` with `visibility`, such as model
    /// and app; empty visibility leaves the specification's default, both. Each linked
    /// tool carries `_meta.ui` naming the App in `tools/list` and tool lookup, and a
    /// tool without a link is never App-callable. Building the routes fails when a link
    /// names an App or a tool the routes do not declare, or links a tool twice.
    pub fn app_tools(mut self, app: &AppUri, tools: &[&str], visibility: &[UiVisibility]) -> Self {
        for tool in tools {
            crate::identity::LocalToolName::parse(tool).expect("valid App-linked tool name");
        }
        self.app_links.push(AppLink {
            app: app.clone(),
            tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
            visibility: visibility.to_vec(),
        });
        self
    }
    pub fn authorization(&self) -> crate::registration::AuthorizationDeclaration {
        let mut requirements = crate::registration::Requirements::default();
        let permissions = self.permission_declarations();
        for (name, route) in &self.tools {
            requirements.tools.insert(
                crate::identity::LocalToolName::parse(name).expect("typed tool name"),
                route.policy.access(),
            );
        }
        for (name, route) in &self.prompts {
            requirements.prompts.insert(
                crate::identity::PromptName::parse(name).expect("typed prompt name"),
                route.policy.access(),
            );
        }
        for route in &self.resources {
            if let Some(fixed) = &route.fixed {
                requirements
                    .resources
                    .insert(fixed.address().clone(), route.policy.access());
            }
            if let Some(template) = &route.template {
                requirements
                    .resource_templates
                    .insert(template.template().clone(), route.policy.access());
            }
        }
        let scheme = <D::Contract as McpServerContract>::scheme();
        let docs = <D::Contract as McpServerContract>::documents();
        for uri in [
            crate::docs::docs_index_uri(&scheme),
            veoveo_types::ResourceUri::new(format!("{}://contract", scheme.as_str()))
                .expect("contract URI"),
        ]
        .into_iter()
        .chain(docs.iter().map(|doc| {
            crate::docs::docs_member_uri(
                &scheme,
                &crate::docs::DocumentId::parse(doc.id).expect("document ID"),
            )
        })) {
            requirements.resources.insert(uri, self.documents.access());
        }
        requirements.resource_templates.insert(
            crate::docs::docs_member_template(&scheme),
            self.documents.access(),
        );
        requirements.app_origin = self.app_origin.as_ref().map(RoutePolicy::access);
        crate::registration::AuthorizationDeclaration {
            permissions: permissions.into_values().collect(),
            requirements,
            uses: self.uses.clone(),
            calls_hub: self.calls_hub,
        }
    }
    pub fn tool<R, H, F>(self, description: &'static str, policy: RoutePolicy, handler: H) -> Self
    where
        R: ToolCall<Provider = <D::Contract as McpServerContract>::Provider>
            + DeserializeOwned
            + JsonSchema,
        R::Output: SingleOutput,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Output, ToolError>> + Send + 'static,
    {
        self.single_tool::<R, H, F>(description, policy, handler, false)
    }
    pub fn task<R, H, F>(self, description: &'static str, policy: RoutePolicy, handler: H) -> Self
    where
        R: crate::calls::TaskToolCall<Provider = <D::Contract as McpServerContract>::Provider>
            + DeserializeOwned
            + JsonSchema,
        R::Output: SingleOutput,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Output, ToolError>> + Send + 'static,
    {
        self.single_tool::<R, H, F>(description, policy, handler, true)
    }
    fn single_tool<R, H, F>(
        mut self,
        description: &'static str,
        policy: RoutePolicy,
        handler: H,
        task: bool,
    ) -> Self
    where
        R: ToolCall<Provider = <D::Contract as McpServerContract>::Provider>
            + DeserializeOwned
            + JsonSchema,
        R::Output: SingleOutput,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Output, ToolError>> + Send + 'static,
    {
        let mut input = rmcp::handler::server::tool::schema_for_type::<R>();
        super::publish_input_schema(Arc::make_mut(&mut input));
        let name = crate::identity::LocalToolName::parse(R::TOOL).expect("valid typed tool name");
        let mut descriptor = Tool::new(name.as_str().to_owned(), description, input);
        descriptor.output_schema =
            Some(rmcp::handler::server::tool::schema_for_type::<R::Output>());
        descriptor.annotations = annotations(&policy);
        let handler = Arc::new(handler);
        let access = policy.clone();
        let invoke: ToolHandler<D> = Arc::new(move |domain, request, context| {
            let handler = handler.clone();
            let access = access.clone();
            Box::pin(async move {
                access.check(&context)?;
                let request = match serde_json::from_value(serde_json::Value::Object(
                    request.arguments.unwrap_or_default(),
                )) {
                    Ok(request) => request,
                    Err(_) => {
                        return Ok(CallToolResult::error(vec![
                            rmcp::model::ContentBlock::text("invalid typed tool arguments"),
                        ]));
                    }
                };
                match handler(domain, request, context).await {
                    Ok(output) => result(&output),
                    Err(ToolError::Protocol(error)) => Err(error),
                    Err(ToolError::Rejected(message)) => Ok(CallToolResult::error(vec![
                        rmcp::model::ContentBlock::text(message),
                    ])),
                }
            })
        });
        assert!(
            self.tools
                .insert(
                    R::TOOL.into(),
                    ToolRoute {
                        descriptor,
                        policy,
                        invoke,
                        task,
                    }
                )
                .is_none(),
            "duplicate typed tool route"
        );
        self
    }
    pub fn collection<T, H, F>(
        mut self,
        description: &'static str,
        policy: RoutePolicy,
        handler: H,
    ) -> Self
    where
        T: CollectionTool<Provider = <D::Contract as McpServerContract>::Provider>,
        <T::Collection as Collection>::Item:
            serde::Serialize + DeserializeOwned + JsonSchema + Send,
        <T::Collection as Collection>::Filters: DeserializeOwned + JsonSchema + Send + Sync,
        H: Fn(D, List<T>, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<Page<T::Collection>, ErrorData>> + Send + 'static,
    {
        let mut input = rmcp::handler::server::tool::schema_for_type::<List<T>>();
        super::publish_input_schema(Arc::make_mut(&mut input));
        let name = crate::identity::LocalToolName::parse(T::TOOL)
            .expect("valid typed collection tool name");
        let mut descriptor = Tool::new(name.as_str().to_owned(), description, input);
        descriptor.output_schema = Some(rmcp::handler::server::tool::schema_for_type::<
            Page<T::Collection>,
        >());
        descriptor.annotations = annotations(&policy);
        let handler = Arc::new(handler);
        let access = policy.clone();
        let invoke: ToolHandler<D> = Arc::new(move |domain, request, context| {
            let handler = handler.clone();
            let access = access.clone();
            Box::pin(async move {
                access.check(&context)?;
                let request = serde_json::from_value(serde_json::Value::Object(
                    request.arguments.unwrap_or_default(),
                ))
                .map_err(|_| ErrorData::invalid_params("invalid collection arguments", None))?;
                let page = handler(domain, request, context).await?;
                result(&page)
            })
        });
        assert!(
            self.tools
                .insert(
                    T::TOOL.into(),
                    ToolRoute {
                        descriptor,
                        policy,
                        invoke,
                        task: false,
                    }
                )
                .is_none(),
            "duplicate typed collection tool"
        );
        self
    }
    pub fn singleton<R, H, F>(
        mut self,
        read: R,
        builder: ResourceBuilder<R, Ready<H>, Ready<RoutePolicy>, Ready<ReadCache>>,
    ) -> Self
    where
        R: ResourceRead<Provider = <D::Contract as McpServerContract>::Provider>
            + Clone
            + Send
            + Sync,
        R::Address: Clone + Eq + Send + Sync + 'static,
        R::Content: SingleOutput,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Content, ErrorData>> + Send + 'static,
    {
        let address = read.address();
        let uri = read.uri().expect("typed singleton address");
        let descriptor = McpResource::new(address.clone(), |uri| {
            Resource::new(uri, builder.name).with_mime_type("application/json")
        })
        .expect("typed singleton descriptor");
        let access = builder.policy.0.clone();
        let policy = access.clone();
        let cache = builder.cache.0;
        let handler = Arc::new(builder.reader.0);
        let invoke: ReadHandler<D> = Arc::new(move |domain, address_requested, context| {
            if <R::Address as ResourceAddress>::parse(&address_requested)
                .ok()
                .as_ref()
                != Some(&address)
            {
                return None;
            }
            let read = read.clone();
            let access = access.clone();
            let handler = handler.clone();
            let uri = uri.clone();
            Some(Box::pin(async move {
                access.check(&context)?;
                let output = handler(domain, read, context).await?;
                Ok((super::results::json_read(uri.as_str(), &output)?, cache))
            }))
        });
        self.insert_resource(ResourceRoute {
            fixed: Some(descriptor),
            template: None,
            policy,
            invoke,
        });
        self
    }
    pub fn template<R, H, F>(
        self,
        builder: ResourceBuilder<R, Ready<H>, Ready<RoutePolicy>, Ready<ReadCache>>,
    ) -> Self
    where
        R: TemplateRead<Provider = <D::Contract as McpServerContract>::Provider> + Send,
        R::Content: SingleOutput,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Content, ErrorData>> + Send + 'static,
    {
        self.resource_template(R::TEMPLATE, R::from_address, builder)
    }
    pub fn collection_resource<R, H, F>(
        mut self,
        root: Option<R>,
        builder: ResourceBuilder<ReadCollection<R>, Ready<H>, Ready<RoutePolicy>, Ready<ReadCache>>,
    ) -> Self
    where
        R: CollectionResource<Provider = <D::Contract as McpServerContract>::Provider> + Send,
        R::Address: Eq,
        <R::Collection as Collection>::Item:
            serde::Serialize + DeserializeOwned + JsonSchema + Send,
        <R::Collection as Collection>::Filters: Send + Sync,
        H: Fn(D, ReadCollection<R>, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<Page<R::Collection>, ErrorData>> + Send + 'static,
    {
        let template = McpResourceTemplate::new(
            crate::definition::collection_template(R::TEMPLATE).expect("typed collection template"),
            |uri| ResourceTemplate::new(uri, builder.name).with_mime_type("application/json"),
        )
        .expect("typed collection descriptor");
        let fixed = root.map(|root| {
            McpResource::new(root.address(), |uri| {
                Resource::new(uri, builder.name).with_mime_type("application/json")
            })
            .expect("typed collection root")
        });
        let access = builder.policy.0.clone();
        let policy = access.clone();
        let cache = builder.cache.0;
        let handler = Arc::new(builder.reader.0);
        let invoke: ReadHandler<D> = Arc::new(move |domain, uri, context| {
            let base = CollectionRequest::<R::Collection>::base_uri(&uri).ok()?;
            let address = R::Address::parse(&base).ok()?;
            let resource = R::from_address(&address)?;
            let selection = CollectionRequest::<R::Collection>::split_uri(&uri, R::default_limit());
            let handler = handler.clone();
            let access = access.clone();
            Some(Box::pin(async move {
                access.check(&context)?;
                let (_, limit, cursor) = selection
                    .map_err(|_| ErrorData::invalid_params("invalid collection selection", None))?;
                let mut read = ReadCollection::new(resource, limit);
                if let Some(cursor) = cursor {
                    read = read.continuing(cursor);
                }
                let output = handler(domain, read, context).await?;
                Ok((super::results::json_read(uri.as_str(), &output)?, cache))
            }))
        });
        self.insert_resource(ResourceRoute {
            fixed,
            template: Some(template),
            policy,
            invoke,
        });
        self
    }
    fn resource_template<R, H, F>(
        mut self,
        template: &str,
        parse: fn(&R::Address) -> Option<R>,
        builder: ResourceBuilder<R, Ready<H>, Ready<RoutePolicy>, Ready<ReadCache>>,
    ) -> Self
    where
        R: ResourceRead<Provider = <D::Contract as McpServerContract>::Provider> + Send,
        H: Fn(D, R, RequestContext<RoleServer>) -> F + Send + Sync + 'static,
        F: Future<Output = Result<R::Content, ErrorData>> + Send + 'static,
    {
        let template = McpResourceTemplate::new(
            ResourceTemplateUri::new(template).expect("typed route template"),
            |uri| ResourceTemplate::new(uri, builder.name).with_mime_type("application/json"),
        )
        .expect("typed template descriptor");
        let access = builder.policy.0.clone();
        let policy = access.clone();
        let cache = builder.cache.0;
        let handler = Arc::new(builder.reader.0);
        let invoke: ReadHandler<D> = Arc::new(move |domain, address, context| {
            let address = R::Address::parse(&address).ok()?;
            let read = parse(&address)?;
            let uri = read.uri().expect("typed member address");
            let handler = handler.clone();
            let access = access.clone();
            Some(Box::pin(async move {
                access.check(&context)?;
                let output = handler(domain, read, context).await?;
                Ok((super::results::json_read(uri.as_str(), &output)?, cache))
            }))
        });
        self.insert_resource(ResourceRoute {
            fixed: None,
            template: Some(template),
            policy,
            invoke,
        });
        self
    }
    pub(crate) fn resources(&self) -> Vec<McpResource> {
        self.resources
            .iter()
            .filter_map(|route| route.fixed.clone())
            .collect()
    }
    pub(crate) fn templates(&self) -> Vec<McpResourceTemplate> {
        self.resources
            .iter()
            .filter_map(|route| route.template.clone())
            .collect()
    }
    pub(crate) fn list_tools(&self, context: &RequestContext<RoleServer>) -> Vec<Tool> {
        self.tools
            .values()
            .filter(|route| route.policy.check(context).is_ok())
            .map(|route| self.current(&route.descriptor))
            .collect()
    }
    pub(crate) fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools
            .get(name)
            .map(|route| self.current(&route.descriptor))
    }
    pub(crate) async fn call(
        &self,
        domain: D,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let route = self
            .tools
            .get(request.name.as_ref())
            .ok_or_else(|| ErrorData::invalid_params("unknown typed tool", None))?;
        (route.invoke)(domain, request, context).await
    }
    pub(crate) async fn read(
        &self,
        domain: D,
        address: veoveo_types::ResourceUri,
        context: RequestContext<RoleServer>,
    ) -> Result<(ReadResourceResult, ReadCache), ErrorData> {
        // Select the route before moving the request context. Route predicates are
        // pure typed address matching; only the selected future performs a read.
        for route in &self.resources {
            if let Some(future) = (route.invoke)(domain.clone(), address.clone(), context.clone()) {
                return future.await;
            }
        }
        Err(ErrorData::invalid_params("unknown typed resource", None))
    }
}

fn result<T: crate::definition::Output>(output: &T) -> Result<CallToolResult, ErrorData> {
    let value = serde_json::to_value(output)
        .map_err(|_| ErrorData::internal_error("typed output serialization failed", None))?;
    let mut result = CallToolResult::structured(value);
    if let Some(uri) = output.product_uri() {
        result.content.insert(
            0,
            rmcp::model::ContentBlock::ResourceLink(Resource::new(uri.as_str(), "Result")),
        );
    }
    Ok(result)
}

/// Serialize a durable tool's terminal value through its registered call contract.
pub fn task_result<R: crate::calls::TaskToolCall>(
    output: &R::Output,
) -> Result<CallToolResult, ErrorData> {
    result(output)
}
