//! MCP Skills (SEP-2640) for hosted servers and their callers.
//!
//! A server's skills are files it embeds at build time from its `skills/` directory
//! (`veoveo_skills::build::embed_skills` in its build script, [`server_skills!`] in its
//! contract). They are resources at `skill://{slug}/{name}/{path}`: the scheme `skill`,
//! the server's slug as the authority, the skill's name and the file's path in the skill.
//! The slug keeps the originating server visible behind the one MCP server a host sees,
//! as `ui://{slug}/` does for Apps, and registration admits a server's requirements on
//! `skill://` only under its own slug. `skill://veoveo/` is the Hub's own skills', which
//! no server holds.
use std::fmt;

use veoveo_types::{ResourceAddress, ResourceUri, ResourceUriParts, ServerSlug};

#[cfg(feature = "server")]
mod served;
#[cfg(feature = "server")]
pub use served::{ServerSkills, ServerSkillsError};

/// The scheme of every skill file.
pub const SCHEME: &str = "skill";
/// The authority of the Hub's own skills, which no server's slug may take.
pub const HUB_AUTHORITY: &str = "veoveo";
/// The file that names a skill.
pub const SKILL_MD: &str = "SKILL.md";

/// A skill file's address, `skill://{authority}/{name}/{path}` in canonical form: the
/// authority a server's slug or [`HUB_AUTHORITY`], a skill name of the Agent Skills
/// format, and a path in the skill of one or more nonempty segments, none `.` or `..`,
/// without query or fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillUri {
    uri: ResourceUri,
    authority: String,
    name: String,
    path: String,
}

/// Why a value is not a skill file's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a skill file's address is skill://{{slug}}/{{name}}/{{path}} in canonical form: {0}")]
pub struct SkillUriError(&'static str);

impl SkillUri {
    pub fn parse(value: &str) -> Result<Self, SkillUriError> {
        let parts =
            ResourceUriParts::parse(value).map_err(|_| SkillUriError("not a canonical URI"))?;
        if parts.scheme() != SCHEME {
            return Err(SkillUriError("the scheme is skill"));
        }
        let authority = parts.authority();
        if authority != HUB_AUTHORITY && ServerSlug::parse(authority).is_err() {
            return Err(SkillUriError("the authority is a server slug"));
        }
        if parts.has_query() {
            return Err(SkillUriError("a skill file's address carries no query"));
        }
        let segments: Vec<_> = parts.path_segments().collect();
        let Some((name, path)) = segments.split_first() else {
            return Err(SkillUriError("the path names a skill and a file in it"));
        };
        if !veoveo_skills::valid_skill_name(name) {
            return Err(SkillUriError("the first segment is a skill name"));
        }
        if path.is_empty()
            || path
                .iter()
                .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
        {
            return Err(SkillUriError(
                "the file is one or more nonempty segments, none . or ..",
            ));
        }
        let uri = ResourceUri::new(value).map_err(|_| SkillUriError("not a resource URI"))?;
        Ok(Self {
            uri,
            authority: authority.to_owned(),
            name: name.to_string(),
            path: path.join("/"),
        })
    }

    /// The address of `path` in skill `name` of server `server`.
    pub fn of(server: &ServerSlug, name: &str, path: &str) -> Result<Self, SkillUriError> {
        Self::parse(&format!("{SCHEME}://{server}/{name}/{path}"))
    }

    /// The server whose skill this is, or `None` for the Hub's own.
    pub fn server(&self) -> Option<ServerSlug> {
        (self.authority != HUB_AUTHORITY)
            .then(|| ServerSlug::parse(&self.authority).expect("a parsed server slug"))
    }

    /// Whether this is one of the Hub's own skills.
    pub fn is_hub(&self) -> bool {
        self.authority == HUB_AUTHORITY
    }

    /// The skill's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file's path in the skill, `/`-separated.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether this is the skill's `SKILL.md`, the address of the skill itself.
    pub fn is_skill_md(&self) -> bool {
        self.path == SKILL_MD
    }

    pub fn as_str(&self) -> &str {
        self.uri.as_str()
    }

    pub fn resource_uri(&self) -> &ResourceUri {
        &self.uri
    }
}

impl fmt::Display for SkillUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ResourceAddress for SkillUri {
    type Error = SkillUriError;

    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        Self::parse(uri.as_str())
    }

    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        Ok(self.uri.clone())
    }
}

/// Whether `uri` is a skill file of `server`: `skill://{server}/…`, never the Hub's.
pub fn owned_by(uri: &str, server: &ServerSlug) -> bool {
    server.as_str() != HUB_AUTHORITY
        && SkillUri::parse(uri).is_ok_and(|uri| uri.server().as_ref() == Some(server))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_addresses_name_a_server_a_skill_and_a_file() {
        let uri = SkillUri::parse("skill://console/watch/SKILL.md").unwrap();
        assert_eq!(uri.server().unwrap().as_str(), "console");
        assert_eq!((uri.name(), uri.path()), ("watch", "SKILL.md"));
        assert!(uri.is_skill_md() && !uri.is_hub());
        let nested = SkillUri::parse("skill://console/watch/references/logs.md").unwrap();
        assert_eq!(nested.path(), "references/logs.md");
        let hub = SkillUri::parse("skill://veoveo/home/SKILL.md").unwrap();
        assert!(hub.is_hub() && hub.server().is_none());
        for invalid in [
            "ui://console/watch/SKILL.md",
            "skill://console",
            "skill://console/watch",
            "skill://console/watch/",
            "skill://console/Watch/SKILL.md",
            "skill://console/watch/../SKILL.md",
            "skill://console/watch/./SKILL.md",
            "skill://console/watch/SKILL.md?x=1",
            "skill://Console/watch/SKILL.md",
        ] {
            assert!(SkillUri::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn a_server_owns_only_its_own_skills() {
        let console = ServerSlug::parse("console").unwrap();
        assert!(owned_by("skill://console/watch/SKILL.md", &console));
        assert!(!owned_by("skill://time/watch/SKILL.md", &console));
        assert!(!owned_by("skill://veoveo/home/SKILL.md", &console));
        if let Ok(veoveo) = ServerSlug::parse(HUB_AUTHORITY) {
            assert!(!owned_by("skill://veoveo/home/SKILL.md", &veoveo));
        }
    }
}
