//! Checked MCP setup from domain-owned contracts. No domain registry lives here.
use std::{collections::BTreeSet, fmt, marker::PhantomData};

use rmcp::model::{Resource, ResourceTemplate, ServerConfig};
use veoveo_types::{
    ResourceAddress, ResourceScheme, ResourceTemplateUri, ResourceUriBuilder, ScopeDefinition,
    ScopeName, UriAuthority, UriSegment,
};

use crate::docs::{DOC_ID_AGENTS, DOC_ID_DESIGN, ServerDocs};

/// MCP associations implemented in a server's MCP feature, above its pure contract.
/// An empty scope vocabulary is valid. This trait establishes API structure, not
/// authentication, domain authorization, recovery, or behavioral compliance.
pub trait McpServerContract: Sized + 'static {
    type Server: crate::hosting::DomainServer<Contract = Self>;
    type Provider: crate::calls::ProviderContract;
    type Scope: ScopeDefinition + 'static;

    fn scheme() -> ResourceScheme;
    fn scopes() -> &'static [Self::Scope];
    fn documents() -> &'static ServerDocs;
    fn server_config() -> ServerConfig;
}

/// The provider identity shared by typed requests and their hosted routes.
pub fn provider_slug<C: McpServerContract>() -> veoveo_types::ServerSlug {
    <C::Provider as crate::calls::ProviderContract>::slug()
}

/// One protocol descriptor bound to its domain address. The builder can reuse
/// ordinary RMCP and App-extension metadata helpers but cannot change the URI.
/// ```compile_fail
/// use veoveo_hub_sdk::server_contract::McpResource;
/// use rmcp::model::Resource;
/// McpResource::new("example://items", |uri| Resource::new(uri, "items"));
/// ```
#[derive(Debug, Clone)]
pub struct McpResource {
    address: veoveo_types::ResourceUri,
    descriptor: Resource,
}

impl McpResource {
    pub(crate) fn new<A: ResourceAddress + Eq>(
        address: A,
        describe: impl FnOnce(&str) -> Resource,
    ) -> Result<Self, McpSetupError> {
        let uri = address
            .to_uri()
            .map_err(|_| McpSetupError::InvalidResource)?;
        uri.components()
            .map_err(|_| McpSetupError::InvalidResource)?;
        if A::parse(&uri).map_err(|_| McpSetupError::InvalidResource)? != address {
            return Err(McpSetupError::ResourceRoundTrip);
        }
        let descriptor = describe(uri.as_str());
        if descriptor.uri != uri.as_str() {
            return Err(McpSetupError::DescriptorAddressMismatch);
        }
        if descriptor.name.trim().is_empty() {
            return Err(McpSetupError::MissingResourceName);
        }
        Ok(Self {
            address: uri,
            descriptor,
        })
    }

    fn host(address: veoveo_types::ResourceUri, name: &str) -> Result<Self, McpSetupError> {
        address
            .components()
            .map_err(|_| McpSetupError::InvalidResource)?;
        Ok(Self {
            descriptor: Resource::new(address.as_str(), name.to_owned()),
            address,
        })
    }
    pub fn address(&self) -> &veoveo_types::ResourceUri {
        &self.address
    }

    pub fn descriptor(&self) -> &Resource {
        &self.descriptor
    }
}

/// A descriptor whose template has passed RFC 6570 admission.
/// ```compile_fail
/// use veoveo_hub_sdk::server_contract::McpResourceTemplate;
/// use veoveo_types::ResourceUri;
/// use rmcp::model::ResourceTemplate;
/// McpResourceTemplate::new(ResourceUri::new("example://items/one").unwrap(),
///     |uri| ResourceTemplate::new(uri, "Items"));
/// ```
#[derive(Debug, Clone)]
pub struct McpResourceTemplate {
    template: ResourceTemplateUri,
    descriptor: ResourceTemplate,
}

impl McpResourceTemplate {
    pub(crate) fn new(
        template: ResourceTemplateUri,
        describe: impl FnOnce(&str) -> ResourceTemplate,
    ) -> Result<Self, McpSetupError> {
        let descriptor = describe(template.as_str());
        if descriptor.uri_template != template.as_str() {
            return Err(McpSetupError::DescriptorTemplateMismatch);
        }
        if descriptor.name.trim().is_empty() {
            return Err(McpSetupError::InvalidTemplate);
        }
        Ok(Self {
            template,
            descriptor,
        })
    }
    pub fn template(&self) -> &ResourceTemplateUri {
        &self.template
    }
    pub fn descriptor(&self) -> &ResourceTemplate {
        &self.descriptor
    }
}

/// A validated setup consumed by hosted handlers for initialization and discovery.
/// Construction requires typed resources and verifies the well-known documents.
/// Resource reads and tool handlers still enforce the caller's current policy.
pub struct McpServerSetup<C: McpServerContract> {
    info: ServerConfig,
    resources: Vec<McpResource>,
    templates: Vec<McpResourceTemplate>,
    scopes: BTreeSet<ScopeName>,
    documents: &'static ServerDocs,
    contract: PhantomData<fn() -> C>,
}

impl<C: McpServerContract> McpServerSetup<C> {
    pub fn new() -> Result<Self, McpSetupError> {
        let slug = provider_slug::<C>();
        let scheme = C::scheme();
        let mut info = C::server_config();
        let documents = C::documents();
        if info.server_info.name != slug.as_str() || documents.server() != slug.as_str() {
            return Err(McpSetupError::IdentityMismatch);
        }
        let native = rmcp::model::ServerCapabilities::builder()
            .enable_resources()
            .enable_completions()
            .enable_tools()
            .enable_prompts()
            .build();
        if info.capabilities.resources.is_none() {
            info.capabilities.resources = native.resources;
        }
        if info.capabilities.completions.is_none() {
            info.capabilities.completions = native.completions;
        }
        use crate::hosting::DomainServer;
        if C::Server::routes().has_tools() {
            if info.capabilities.tools.is_none() {
                info.capabilities.tools = native.tools;
            }
            // Descriptions that change while the server runs change its tool list.
            if C::Server::routes().descriptions().is_some()
                && let Some(tools) = info.capabilities.tools.as_mut()
            {
                tools.list_changed = Some(true);
            }
        } else {
            info.capabilities.tools = None;
        }
        if C::Server::routes().has_prompts() {
            if info.capabilities.prompts.is_none() {
                info.capabilities.prompts = native.prompts;
            }
        } else {
            info.capabilities.prompts = None;
        }
        if C::Server::routes().has_tasks() {
            info.capabilities.extensions.get_or_insert_default().insert(
                rmcp::model::TASKS_EXTENSION_ID.to_owned(),
                rmcp::model::JsonObject::new(),
            );
        }
        let extensions = info.capabilities.extensions.get_or_insert_default();
        extensions.remove(crate::registration::AUTHORIZATION_EXTENSION_ID);
        if extensions.contains_key(crate::registration::REGISTRATION_EXTENSION_ID) {
            extensions.insert(
                crate::registration::AUTHORIZATION_EXTENSION_ID.to_owned(),
                match serde_json::to_value(C::Server::routes().authorization())
                    .expect("typed route authorization serializes")
                {
                    serde_json::Value::Object(object) => object,
                    _ => unreachable!("authorization is an object"),
                },
            );
        }
        let mut scopes = BTreeSet::new();
        for scope in C::scopes() {
            if !scopes.insert(scope.name().clone()) {
                return Err(McpSetupError::DuplicateScope);
            }
        }
        let mut resources = C::Server::routes().resources();
        for (root, name) in [("docs", "Documents"), ("contract", "Contract")] {
            let uri = well_known_builder(&scheme, root)?
                .build()
                .map_err(|_| McpSetupError::InvalidDocument)?;
            resources.push(McpResource::host(uri, name)?);
        }
        for doc in documents.iter() {
            let uri = well_known_builder(&scheme, "docs")?
                .segment(UriSegment::new(doc.id).map_err(|_| McpSetupError::InvalidDocument)?)
                .build()
                .map_err(|_| McpSetupError::InvalidDocument)?;
            resources.push(McpResource::host(uri, doc.title)?);
        }
        resources.sort_by(|a, b| a.descriptor.uri.cmp(&b.descriptor.uri));
        let mut addresses = BTreeSet::new();
        for resource in &resources {
            if !addresses.insert(resource.descriptor.uri.as_str()) {
                return Err(McpSetupError::DuplicateResource);
            }
        }
        for id in [DOC_ID_AGENTS, DOC_ID_DESIGN] {
            if documents
                .doc(id)
                .is_none_or(|doc| doc.body.trim().is_empty())
            {
                return Err(McpSetupError::MissingDocument);
            }
        }
        let mut document_ids = BTreeSet::new();
        for doc in documents.iter() {
            if doc.body.trim().is_empty() {
                return Err(McpSetupError::MissingDocument);
            }
            if !document_ids.insert(doc.id) {
                return Err(McpSetupError::InvalidDocument);
            }
        }
        let mut templates = C::Server::routes().templates();
        templates.push(McpResourceTemplate::new(
            crate::docs::docs_member_template(&scheme),
            |uri| ResourceTemplate::new(uri, "Document"),
        )?);
        templates.sort_by(|a, b| a.template.cmp(&b.template));
        let mut declared_templates = BTreeSet::new();
        for template in &templates {
            if !declared_templates.insert(template.template.as_str()) {
                return Err(McpSetupError::DuplicateTemplate);
            }
        }
        Ok(Self {
            info,
            resources,
            templates,
            scopes,
            documents,
            contract: PhantomData,
        })
    }

    pub fn server_config(&self) -> &ServerConfig {
        &self.info
    }
    pub fn resources(&self) -> &[McpResource] {
        &self.resources
    }
    pub fn resource_templates(&self) -> &[McpResourceTemplate] {
        &self.templates
    }
    pub fn documents(&self) -> &'static ServerDocs {
        self.documents
    }

    /// Serves the well-known `{scheme}://docs` routes and `{scheme}://contract`.
    /// Returns `None` for every other address, which the domain then reads.
    pub fn read_documents(
        &self,
        request: &rmcp::model::ReadResourceRequestParams,
        context: &rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<Option<rmcp::model::ReadResourceResponse>, rmcp::ErrorData> {
        if let Some(result) = self.read_contract(request, context)? {
            return Ok(Some(result));
        }
        self.documents.read_docs(&C::scheme(), request, context)
    }

    fn read_contract(
        &self,
        request: &rmcp::model::ReadResourceRequestParams,
        context: &rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<Option<rmcp::model::ReadResourceResponse>, rmcp::ErrorData> {
        let contract = well_known_builder(&C::scheme(), "contract")
            .and_then(|builder| builder.build().map_err(|_| McpSetupError::InvalidDocument))
            .map_err(|_| rmcp::ErrorData::internal_error("invalid contract address", None))?;
        if request.uri != contract.as_str() {
            return Ok(None);
        }
        crate::hosting::hub_identity(context)?;
        let text = serde_json::to_string(&self.documents.contract_declaration()).map_err(|_| {
            rmcp::ErrorData::internal_error("contract declaration serialization failed", None)
        })?;
        let result = rmcp::model::ReadResourceResult::new(vec![
            rmcp::model::ResourceContents::text(text, &request.uri)
                .with_mime_type("application/json"),
        ]);
        let cacheable = request.request_state.is_none() && request.input_responses.is_none();
        Ok(Some(crate::private_resource_response(result, cacheable)))
    }
    pub fn scope_names(&self) -> &BTreeSet<ScopeName> {
        &self.scopes
    }

    /// Compares a domain scope with an already authenticated grant set. This
    /// membership test does not authenticate the set or authorize a resource.
    /// ```compile_fail
    /// use veoveo_hub_sdk::server_contract::{McpServerContract, McpServerSetup};
    /// use std::collections::BTreeSet;
    /// fn check<C: McpServerContract>(setup: &McpServerSetup<C>) {
    ///     setup.has_scope(&BTreeSet::new(), "example:read");
    /// }
    /// ```
    /// A different server's enum cannot be used as this server's permission.
    /// ```compile_fail
    /// use veoveo_hub_sdk::server_contract::{McpServerContract, McpServerSetup};
    /// use std::collections::BTreeSet;
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
    /// #[vocabulary(scope)]
    /// enum ForeignScope { #[vocabulary(rename = "foreign:read")]
    /// Read
    /// }
    /// fn check<C: McpServerContract>(setup: &McpServerSetup<C>) {
    ///     setup.has_scope(&BTreeSet::new(), ForeignScope::Read);
    /// }
    /// ```
    pub fn has_scope(&self, grants: &BTreeSet<ScopeName>, required: C::Scope) -> bool {
        self.scopes.contains(required.name()) && grants.contains(required.name())
    }
}

fn well_known_builder(
    scheme: &ResourceScheme,
    authority: &str,
) -> Result<ResourceUriBuilder, McpSetupError> {
    ResourceUriBuilder::from_components(
        scheme,
        UriAuthority::new(authority).map_err(|_| McpSetupError::InvalidDocument)?,
    )
    .map_err(|_| McpSetupError::InvalidDocument)
}

/// Errors describe setup invariants without copying resource identities or values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSetupError {
    IdentityMismatch,
    InvalidResource,
    ResourceRoundTrip,
    DescriptorAddressMismatch,
    MissingResourceName,
    DuplicateResource,
    DuplicateScope,
    MissingDocument,
    InvalidDocument,
    InvalidTemplate,
    DescriptorTemplateMismatch,
    DuplicateTemplate,
}

impl fmt::Display for McpSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IdentityMismatch => {
                "server slug, implementation name and document owner must agree"
            }
            Self::InvalidResource => "resource address must build and parse as a concrete URI",
            Self::ResourceRoundTrip => {
                "resource address does not round-trip through its owning parser"
            }
            Self::DescriptorAddressMismatch => {
                "resource metadata builder changed the typed address"
            }
            Self::MissingResourceName => "resource descriptor requires a nonblank name",
            Self::DuplicateResource => "resource discovery contains a duplicate address",
            Self::DuplicateScope => "scope vocabulary contains duplicate names",
            Self::MissingDocument => {
                "hosted server requires agent and design documents and nonempty document bodies"
            }
            Self::InvalidDocument => "document identifiers must be unique valid URI path segments",
            Self::InvalidTemplate => {
                "resource template requires valid absolute RFC 6570 syntax and a nonblank name"
            }
            Self::DescriptorTemplateMismatch => {
                "resource metadata builder changed the typed template"
            }
            Self::DuplicateTemplate => "resource discovery contains a duplicate template",
        })
    }
}
impl std::error::Error for McpSetupError {}
