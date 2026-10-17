//! Server namespace projection for hub tool identities.

use crate::identity::LocalToolName;
use veoveo_types::{IdentifierError, ServerSlug};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(transparent)]
pub struct HubToolNameError(#[from] IdentifierError);

/// Hub-scoped tool name after server namespace projection.
#[veoveo_types::id(text(ToolNames))]
pub struct HubToolName(String);
impl HubToolName {
    /// Resolve the admitted namespace into typed owner and local identities.
    /// Unqualified names remain admitted by this wire scalar but have no projection.
    pub fn parts(&self) -> Result<(ServerSlug, LocalToolName), HubToolNamespaceError> {
        let (server, tool) = self
            .as_str()
            .split_once("__")
            .ok_or(HubToolNamespaceError::Missing)?;
        if tool.contains("__") {
            return Err(HubToolNamespaceError::Ambiguous);
        }
        Ok((
            ServerSlug::parse(server).map_err(HubToolNamespaceError::Server)?,
            LocalToolName::parse(tool).map_err(HubToolNamespaceError::Tool)?,
        ))
    }

    /// Compose the hub namespace from typed server and local tool names.
    pub fn from_parts(server: &ServerSlug, tool: &LocalToolName) -> Result<Self, HubToolNameError> {
        Self::parse(format!("{server}__{tool}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubToolNamespaceError {
    #[error("hub tool name has no server namespace")]
    Missing,
    #[error("hub tool name has multiple namespace delimiters")]
    Ambiguous,
    #[error("invalid server namespace: {0}")]
    Server(IdentifierError),
    #[error("invalid local tool identity: {0}")]
    Tool(IdentifierError),
}

#[doc(hidden)]
pub struct ToolNames;
impl veoveo_types::IdProfile for ToolNames {
    type Error = HubToolNameError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(|value, _| validate_tool_name(value));
}

fn validate_tool_name(value: &str) -> Result<(), HubToolNameError> {
    if value.is_empty() {
        return Err(IdentifierError::new(value, "must not be empty").into());
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(IdentifierError::new(
            value,
            "must contain only lowercase ASCII letters, digits, hyphen, or underscore",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hub_tool_names_preserve_namespace_parser_and_string_schema() {
        let projected = HubToolName::from_parts(
            &ServerSlug::parse("media").unwrap(),
            &LocalToolName::parse("render").unwrap(),
        )
        .unwrap();
        assert_eq!(projected.to_string(), "media__render");
        assert_eq!(
            projected.parts().unwrap(),
            (
                ServerSlug::parse("media").unwrap(),
                LocalToolName::parse("render").unwrap()
            )
        );
        assert!(HubToolName::parse("plain").unwrap().parts().is_err());
        assert!(
            HubToolName::parse("media__run__extra")
                .unwrap()
                .parts()
                .is_err()
        );
        assert_eq!(serde_json::to_value(&projected).unwrap(), "media__render");
        for valid in ["plain", "under_score", "dash-1", "media__render"] {
            assert_eq!(valid.parse::<HubToolName>().unwrap().as_str(), valid);
        }
        for invalid in [
            "",
            "Uppercase",
            "media.render",
            "media/render",
            " spaced",
            "é",
        ] {
            assert!(HubToolName::parse(invalid).is_err());
            assert!(invalid.parse::<HubToolName>().is_err());
            assert!(serde_json::from_value::<HubToolName>(serde_json::json!(invalid)).is_err());
        }
        let schema = serde_json::to_value(schemars::schema_for!(HubToolName)).unwrap();
        assert_eq!(schema["title"], "HubToolName");
        assert_eq!(schema["type"], "string");
        assert!(schema.get("pattern").is_none());
    }
}
