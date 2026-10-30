//! MCP discovery: a server advertises the same declarations it registers in
//! `capabilities.extensions`. This module supplies advertisement and typed readback
//! helpers; the Hub uses the checked stored registration for policy, without an
//! automatic comparison against the discovery advertisement.
use rmcp::model::{JsonObject, ServerCapabilities};

use crate::registration::{
    AUTHORIZATION_EXTENSION_ID, AuthorizationDeclaration, DeclarationError,
    REGISTRATION_EXTENSION_ID, RegistrationDeclaration,
};

/// Advertise the declarations.
pub fn advertise(
    capabilities: &mut ServerCapabilities,
    registration: &RegistrationDeclaration,
    authorization: &AuthorizationDeclaration,
) {
    let extensions = capabilities.extensions.get_or_insert_default();
    extensions.insert(REGISTRATION_EXTENSION_ID.into(), object(registration));
    extensions.insert(AUTHORIZATION_EXTENSION_ID.into(), object(authorization));
}

fn object(value: &impl serde::Serialize) -> JsonObject {
    match serde_json::to_value(value).expect("typed declaration serializes") {
        serde_json::Value::Object(object) => object,
        _ => unreachable!("declarations serialize as JSON objects"),
    }
}

/// What a server advertises in its discovery result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advertised {
    pub registration: RegistrationDeclaration,
    pub authorization: AuthorizationDeclaration,
}

pub fn advertised(
    capabilities: &ServerCapabilities,
) -> Result<Option<Advertised>, DeclarationError> {
    let Some(extensions) = capabilities.extensions.as_ref() else {
        return Ok(None);
    };
    let registration: Option<RegistrationDeclaration> =
        read(extensions, REGISTRATION_EXTENSION_ID, "registration")?;
    let authorization: Option<AuthorizationDeclaration> =
        read(extensions, AUTHORIZATION_EXTENSION_ID, "authorization")?;
    match (registration, authorization) {
        (None, None) => Ok(None),
        (Some(registration), Some(authorization)) => Ok(Some(Advertised {
            registration,
            authorization,
        })),
        _ => Err(DeclarationError::invalid(
            "capabilities",
            "registration and authorization must be advertised together",
        )),
    }
}

fn read<T: serde::de::DeserializeOwned>(
    extensions: &rmcp::model::ExtensionCapabilities,
    id: &str,
    field: &'static str,
) -> Result<Option<T>, DeclarationError> {
    extensions
        .get(id)
        .map(|object| {
            serde_json::from_value(serde_json::Value::Object(object.clone()))
                .map_err(|_| DeclarationError::invalid(field, "does not match its declared shape"))
        })
        .transpose()
}
