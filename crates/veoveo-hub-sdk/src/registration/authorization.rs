//! `ai.veoveo/authorization`: the permissions a server declares, the permission each of
//! its tools, resources, resource templates and prompts requires, and the permissions it
//! uses on other servers.
use std::collections::{BTreeMap, BTreeSet};

use crate::identity::{LocalToolName, PromptName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::{
    PermissionName, ResourceScheme, ResourceTemplateUri, ResourceTypeName, ResourceUri, ServerSlug,
    Vocabulary,
};

use crate::registration::{
    Access, DeclarationError, Risk, authorization_template::AuthorizationTemplate,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionDeclaration {
    pub permission: PermissionName,
    pub resource: ResourceTypeName,
    pub access: Access,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub risks: BTreeSet<Risk>,
}

/// Presence declares a route. Authenticated never means anonymous. A missing
/// entry remains undeclared, distinct from an explicitly admitted caller route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "permission",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RouteAccess {
    Authenticated,
    Permission(PermissionName),
}
impl RouteAccess {
    pub fn permission(&self) -> Option<&PermissionName> {
        match self {
            Self::Authenticated => None,
            Self::Permission(permission) => Some(permission),
        }
    }
}

/// Each target declares its access policy. An omitted target is undeclared.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<LocalToolName, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<ResourceUri, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_templates: BTreeMap<ResourceTemplateUri, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prompts: BTreeMap<PromptName, RouteAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizationDeclaration {
    pub permissions: Vec<PermissionDeclaration>,
    pub requirements: Requirements,
    /// Permissions of other servers this server calls with, each in its provider's
    /// namespace. A call between servers carries at most these, narrowed to what the
    /// caller holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<PermissionName>,
    /// Whether this server calls the Hub's own routes for its caller, such as
    /// `PUT /selection/{profile}/{server}`, with a token the Hub issues it in a token
    /// exchange naming no permissions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub calls_hub: bool,
}

/// [`AuthorizationDeclaration`] as the `veoveo_authorization` registration member
/// spells it: OAuth documents use snake_case member names.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AuthorizationDocument")]
pub(crate) struct AuthorizationDocument {
    permissions: Vec<PermissionDeclaration>,
    requirements: RequirementsDocument,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    uses: Vec<PermissionName>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    calls_hub: bool,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RequirementsDocument {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tools: BTreeMap<LocalToolName, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    resources: BTreeMap<ResourceUri, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    resource_templates: BTreeMap<ResourceTemplateUri, RouteAccess>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    prompts: BTreeMap<PromptName, RouteAccess>,
}

impl From<AuthorizationDocument> for AuthorizationDeclaration {
    fn from(document: AuthorizationDocument) -> Self {
        let requirements = document.requirements;
        Self {
            permissions: document.permissions,
            requirements: Requirements {
                tools: requirements.tools,
                resources: requirements.resources,
                resource_templates: requirements.resource_templates,
                prompts: requirements.prompts,
            },
            uses: document.uses,
            calls_hub: document.calls_hub,
        }
    }
}
impl From<AuthorizationDeclaration> for AuthorizationDocument {
    fn from(declaration: AuthorizationDeclaration) -> Self {
        let requirements = declaration.requirements;
        Self {
            permissions: declaration.permissions,
            requirements: RequirementsDocument {
                tools: requirements.tools,
                resources: requirements.resources,
                resource_templates: requirements.resource_templates,
                prompts: requirements.prompts,
            },
            uses: declaration.uses,
            calls_hub: declaration.calls_hub,
        }
    }
}

/// One request target whose required permission the Hub looks up.
#[derive(Debug, Clone, Copy)]
pub enum RequirementTarget<'a> {
    Tool(&'a LocalToolName),
    Resource(&'a ResourceUri),
    ResourceTemplate(&'a ResourceTemplateUri),
    Prompt(&'a PromptName),
}

impl AuthorizationDeclaration {
    /// Check a declaration against the namespace pinned to the registering server.
    pub fn check(
        &self,
        server: &ServerSlug,
        uri_scheme: &ResourceScheme,
    ) -> Result<(), DeclarationError> {
        let mut declared = BTreeSet::new();
        for permission in &self.permissions {
            if !permission.permission.is_owned_by(server) {
                return Err(DeclarationError::PermissionOutsideNamespace {
                    permission: permission.permission.clone(),
                });
            }
            if !permission.resource.is_owned_by(server) {
                return Err(DeclarationError::ResourceOutsideNamespace {
                    resource: permission.resource.clone(),
                });
            }
            if !declared.insert(&permission.permission) {
                return Err(DeclarationError::DuplicatePermission {
                    permission: permission.permission.clone(),
                });
            }
        }
        let requirements = &self.requirements;
        for required in requirements
            .tools
            .values()
            .chain(requirements.resources.values())
            .chain(requirements.resource_templates.values())
            .chain(requirements.prompts.values())
            .filter_map(RouteAccess::permission)
        {
            if !declared.contains(required) {
                return Err(DeclarationError::UndeclaredPermission {
                    permission: required.clone(),
                });
            }
        }
        for uri in requirements.resources.keys() {
            if !owns_uri(server, uri_scheme, uri.as_str()) {
                return Err(DeclarationError::ResourceOutsideScheme {
                    uri: uri.as_str().to_owned(),
                });
            }
        }
        for template in requirements.resource_templates.keys() {
            if !owns_uri(server, uri_scheme, template.as_str()) {
                return Err(DeclarationError::ResourceOutsideScheme {
                    uri: template.as_str().to_owned(),
                });
            }
            if AuthorizationTemplate::parse(template).is_none() {
                return Err(DeclarationError::UnsupportedTemplate {
                    template: template.as_str().to_owned(),
                });
            }
        }
        for (prompt, permission) in &requirements.prompts {
            if permission
                .permission()
                .and_then(|permission| self.permission(permission))
                .is_some_and(|declared| declared.access == Access::Write)
            {
                return Err(DeclarationError::PromptRequiresWrite {
                    prompt: prompt.clone(),
                });
            }
        }
        let mut used = BTreeSet::new();
        for permission in &self.uses {
            if permission.is_owned_by(server) {
                return Err(DeclarationError::UseInsideNamespace {
                    permission: permission.clone(),
                });
            }
            if !used.insert(permission) {
                return Err(DeclarationError::DuplicateUse {
                    permission: permission.clone(),
                });
            }
        }
        Ok(())
    }

    /// The declared uses in `provider`'s namespace.
    pub fn uses_of<'a>(
        &'a self,
        provider: &'a ServerSlug,
    ) -> impl Iterator<Item = &'a PermissionName> + 'a {
        self.uses
            .iter()
            .filter(move |permission| permission.is_owned_by(provider))
    }

    pub fn permission(&self, name: &PermissionName) -> Option<&PermissionDeclaration> {
        self.permissions
            .iter()
            .find(|permission| &permission.permission == name)
    }

    /// The permission a target requires. An exact resource entry wins over templates;
    /// a resource matched by templates that require different permissions has none.
    pub fn access(&self, target: RequirementTarget<'_>) -> Option<&RouteAccess> {
        let requirements = &self.requirements;
        let name = match target {
            RequirementTarget::Tool(tool) => requirements.tools.get(tool),
            RequirementTarget::Prompt(prompt) => requirements.prompts.get(prompt),
            RequirementTarget::ResourceTemplate(template) => {
                requirements.resource_templates.get(template)
            }
            RequirementTarget::Resource(uri) => requirements.resources.get(uri).or_else(|| {
                let mut matched = requirements
                    .resource_templates
                    .iter()
                    .filter(|(template, _)| {
                        AuthorizationTemplate::parse(template)
                            .is_some_and(|template| template.matches(uri))
                    })
                    .map(|(_, permission)| permission);
                let first = matched.next()?;
                matched.all(|other| other == first).then_some(first)
            }),
        }?;
        Some(name)
    }
}

/// A server's resources use its own scheme, or `ui://{slug}/` for its Apps.
fn owns_uri(server: &ServerSlug, scheme: &ResourceScheme, uri: &str) -> bool {
    uri.split_once("://").is_some_and(|(found, rest)| {
        found == scheme.as_str()
            || (found == "ui"
                && rest
                    .strip_prefix(server.as_str())
                    .is_some_and(|tail| tail.starts_with('/')))
    })
}

/// A server's closed permission vocabulary. One wire spelling per variant, such as
/// `time.events.write`; the builder checks each spelling once.
pub trait PermissionDefinition: Vocabulary + Ord {
    /// The resource type this permission acts on, such as `time.event`.
    fn resource(self) -> &'static str;
    fn access(self) -> Access;
    fn risks(self) -> &'static [Risk] {
        &[]
    }
}
