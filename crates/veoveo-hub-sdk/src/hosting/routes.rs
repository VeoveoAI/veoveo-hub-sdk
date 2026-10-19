//! Typed server registration. Raw MCP results are constructed only after a
//! registered request/output association and its current policy have succeeded.
use super::{DomainServer, ReadCache, hub_identity};
use crate::{
    calls::{ResourceRead, ToolCall},
    definition::{
        CollectionRequest, CollectionResource, CollectionTool, List, ReadCollection, SingleOutput,
    },
    registration::{PermissionDeclaration, PermissionDefinition},
    server_contract::{McpResource, McpResourceTemplate, McpServerContract},
};
use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult, Prompt,
        ReadResourceResult, Resource, ResourceTemplate, Tool,
    },
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, future::Future, marker::PhantomData, pin::Pin, sync::Arc};
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
    pub fn builder(documents: RoutePolicy) -> RoutesBuilder<D> {
        RoutesBuilder::new(documents)
    }
    pub fn authorization(&self) -> crate::registration::AuthorizationDeclaration {
        self.inner.authorization()
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
}
impl<D: DomainServer> RoutesBuilder<D> {
    fn new(documents: RoutePolicy) -> Self {
        Self {
            tools: BTreeMap::new(),
            prompts: BTreeMap::new(),
            resources: vec![],
            documents,
            uses: vec![],
        }
    }
    pub fn build(self) -> Routes<D> {
        Routes { inner: self }
    }
    pub fn uses(mut self, permission: PermissionName) -> Self {
        self.uses.push(permission);
        self
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
    pub fn authorization(&self) -> crate::registration::AuthorizationDeclaration {
        let mut requirements = crate::registration::Requirements::default();
        let mut permissions = BTreeMap::new();
        let mut remember = |policy: &RoutePolicy| {
            if let RoutePolicy::Permission(permission) = policy {
                permissions.insert(permission.permission.clone(), permission.clone());
            }
        };
        for (name, route) in &self.tools {
            requirements.tools.insert(
                crate::identity::LocalToolName::parse(name).expect("typed tool name"),
                route.policy.access(),
            );
            remember(&route.policy);
        }
        for (name, route) in &self.prompts {
            requirements.prompts.insert(
                crate::identity::PromptName::parse(name).expect("typed prompt name"),
                route.policy.access(),
            );
            remember(&route.policy);
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
            remember(&route.policy);
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
        remember(&self.documents);
        crate::registration::AuthorizationDeclaration {
            permissions: permissions.into_values().collect(),
            requirements,
            uses: self.uses.clone(),
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
        self.resources.push(ResourceRoute {
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
        self.resources.push(ResourceRoute {
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
        self.resources.push(ResourceRoute {
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
            .map(|route| route.descriptor.clone())
            .collect()
    }
    pub(crate) fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.get(name).map(|route| route.descriptor.clone())
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
