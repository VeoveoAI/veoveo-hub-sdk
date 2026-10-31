//! Server namespace projection for hub tool identities.

use crate::identity::LocalToolName;
use veoveo_types::{IdentifierError, ServerSlug};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(transparent)]
pub struct HubToolNameError(#[from] IdentifierError);

/// Hub-scoped tool name after server namespace projection: `{slug}__{tool}`, or
/// `{slug}__{link}__{tool}` for a server served by the runners of linked repositories.
#[veoveo_types::id(text(ToolNames))]
pub struct HubToolName(String);

/// The parts of a [`HubToolName`]: the server, the link whose runner serves the tool when
/// the server is linked, and the tool's name on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubToolParts {
    pub server: ServerSlug,
    pub link: Option<LinkName>,
    pub tool: LocalToolName,
}

impl HubToolName {
    /// Resolve the admitted namespace of a server that is not linked into typed owner and
    /// local identities. A linked name has no such projection; [`Self::projection`]
    /// resolves it. Unqualified names remain admitted by this wire scalar but have no
    /// projection.
    pub fn parts(&self) -> Result<(ServerSlug, LocalToolName), HubToolNamespaceError> {
        let HubToolParts { server, link, tool } = self.projection()?;
        match link {
            None => Ok((server, tool)),
            Some(_) => Err(HubToolNamespaceError::Linked),
        }
    }

    /// Resolve the admitted namespace into typed owner, link and local identities. A
    /// slug holds no `__`, so the name splits at its first `__`; what follows is the
    /// tool, or the link and the tool when a second `__` follows. Unqualified names
    /// remain admitted by this wire scalar but have no projection.
    pub fn projection(&self) -> Result<HubToolParts, HubToolNamespaceError> {
        let (server, rest) = self
            .as_str()
            .split_once("__")
            .ok_or(HubToolNamespaceError::Missing)?;
        let (link, tool) = match rest.split_once("__") {
            Some((link, tool)) => (Some(link), tool),
            None => (None, rest),
        };
        if tool.contains("__") {
            return Err(HubToolNamespaceError::Ambiguous);
        }
        Ok(HubToolParts {
            server: ServerSlug::parse(server).map_err(HubToolNamespaceError::Server)?,
            link: link
                .map(LinkName::parse)
                .transpose()
                .map_err(HubToolNamespaceError::Link)?,
            tool: LocalToolName::parse(tool).map_err(HubToolNamespaceError::Tool)?,
        })
    }

    /// Compose the hub namespace from typed server and local tool names.
    pub fn from_parts(server: &ServerSlug, tool: &LocalToolName) -> Result<Self, HubToolNameError> {
        Self::parse(format!("{server}__{tool}"))
    }

    /// Compose the name of `tool` as the runner of `link` serves it for `server`.
    pub fn from_linked_parts(
        server: &ServerSlug,
        link: &LinkName,
        tool: &LocalToolName,
    ) -> Result<Self, HubToolNameError> {
        Self::parse(format!("{server}__{link}__{tool}"))
    }
}

/// The name of a link at the top level of a linked server repository's branch: a
/// lowercase DNS label (RFC 1123 §2.1), 1 to 63 of `a`–`z`, `0`–`9` and `-`, starting and
/// ending with a letter or digit. It names the link's runner in tool names and resource
/// URIs.
#[veoveo_types::id(text(LinkNames))]
pub struct LinkName(String);

#[doc(hidden)]
pub struct LinkNames;
impl veoveo_types::IdProfile for LinkNames {
    type Error = IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(|value, _| validate_link_name(value));
}

fn validate_link_name(value: &str) -> Result<(), IdentifierError> {
    let bytes = value.as_bytes();
    let alphanumeric = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes.first().is_some_and(alphanumeric)
        || !bytes.last().is_some_and(alphanumeric)
        || !bytes.iter().all(|b| alphanumeric(b) || *b == b'-')
    {
        return Err(IdentifierError::new(
            value,
            "a link name is a lowercase DNS label: 1 to 63 of a-z, 0-9 and -, starting and \
             ending with a letter or digit",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubToolNamespaceError {
    #[error("hub tool name has no server namespace")]
    Missing,
    #[error("hub tool name has multiple namespace delimiters")]
    Ambiguous,
    #[error("hub tool name names a link of a linked server")]
    Linked,
    #[error("invalid server namespace: {0}")]
    Server(IdentifierError),
    #[error("invalid link: {0}")]
    Link(IdentifierError),
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
        assert_eq!(
            projected.projection().unwrap(),
            HubToolParts {
                server: ServerSlug::parse("media").unwrap(),
                link: None,
                tool: LocalToolName::parse("render").unwrap()
            }
        );
        assert!(HubToolName::parse("plain").unwrap().parts().is_err());
        assert!(
            HubToolName::parse("media__run__extra__more")
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

#[cfg(test)]
mod linked_tests {
    use super::*;

    #[test]
    fn a_linked_name_carries_its_link_between_the_server_and_the_tool() {
        let server = ServerSlug::parse("n8n").unwrap();
        let link = LinkName::parse("team-a").unwrap();
        let tool = LocalToolName::parse("run_workflow").unwrap();
        let name = HubToolName::from_linked_parts(&server, &link, &tool).unwrap();
        assert_eq!(name.as_str(), "n8n__team-a__run_workflow");
        assert_eq!(
            name.projection().unwrap(),
            HubToolParts {
                server,
                link: Some(link),
                tool
            }
        );
        assert_eq!(name.parts(), Err(HubToolNamespaceError::Linked));
        // A link that is no DNS label, and a third delimiter, have no projection.
        assert!(
            HubToolName::parse("n8n__Team__run").is_err(),
            "no uppercase"
        );
        for name in [
            "n8n__-team__run",
            "n8n__team-__run",
            "n8n__team_a__run",
            "n8n____run",
            "n8n__team__run__more",
        ] {
            assert!(
                HubToolName::parse(name).unwrap().projection().is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn a_link_name_is_a_lowercase_dns_label() {
        for valid in ["a", "team-a", "0", "a1b2", &"a".repeat(63)] {
            assert!(LinkName::parse(valid).is_ok(), "{valid:?}");
        }
        for invalid in ["", "-a", "a-", "A", "a_b", "a.b", "a b", &"a".repeat(64)] {
            assert!(LinkName::parse(invalid).is_err(), "{invalid:?}");
        }
    }
}
