use crate::identity::PromptName;
use veoveo_types::{PermissionName, ResourceTypeName};

/// The exact reason a declaration is rejected. Diagnostics name declared identifiers,
/// never request contents or secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeclarationError {
    #[error("permission {permission} is outside the server's namespace")]
    PermissionOutsideNamespace { permission: PermissionName },
    #[error("resource type {resource} is outside the server's namespace")]
    ResourceOutsideNamespace { resource: ResourceTypeName },
    #[error("permission {permission} is declared more than once")]
    DuplicatePermission { permission: PermissionName },
    #[error("a requirement names undeclared permission {permission}")]
    UndeclaredPermission { permission: PermissionName },
    #[error("resource requirement {uri} is outside the server's URI scheme")]
    ResourceOutsideScheme { uri: String },
    #[error(
        "resource template {template} is outside the native path and optional-query route profile"
    )]
    UnsupportedTemplate { template: String },
    #[error("prompt {prompt} requires a write permission")]
    PromptRequiresWrite { prompt: PromptName },
    #[error("use of {permission} names the server's own namespace; declare it as a permission")]
    UseInsideNamespace { permission: PermissionName },
    #[error("use of {permission} is declared more than once")]
    DuplicateUse { permission: PermissionName },
    #[error("invalid {field}: {reason}")]
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
}

impl DeclarationError {
    pub(crate) fn invalid(field: &'static str, reason: &'static str) -> Self {
        Self::Invalid { field, reason }
    }
}
