//! A server's embedded skills, as `skills/list`, `skills/get` and `resources/read` serve
//! them.
use std::collections::BTreeMap;

use base64::Engine as _;
use rmcp::{
    ErrorData,
    model::{
        GetSkillResult, ListSkillsResult, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, ResourceContents, Skill, SkillFrontmatter, SkillResource,
        SkillResources,
    },
};
use veoveo_skills::{EmbeddedFile, EmbeddedSkill};
use veoveo_types::ServerSlug;

use super::{HUB_AUTHORITY, SKILL_MD, SkillUri};

/// The skills a server serves: each as its `skills/list` entry, the frontmatter its
/// author wrote and every file with its digest and size, and the files by address.
#[derive(Debug)]
pub struct ServerSkills {
    server: Option<ServerSlug>,
    skills: Vec<Skill>,
    files: BTreeMap<String, &'static EmbeddedFile>,
}

/// Why a server's embedded skills cannot be served.
#[derive(Debug, thiserror::Error)]
pub enum ServerSkillsError {
    #[error("{0} is the Hub's own skill authority; no server's skills take it")]
    ReservedAuthority(ServerSlug),
    #[error("skill {skill}: {reason}")]
    Invalid { skill: String, reason: String },
}

impl ServerSkills {
    /// A server without skills.
    pub const fn none() -> Self {
        Self {
            server: None,
            skills: Vec::new(),
            files: BTreeMap::new(),
        }
    }

    /// The skills of `server` embedded as `table`, the build script's
    /// `veoveo_skills::build::embed_skills` output.
    pub fn embedded(
        server: ServerSlug,
        table: &'static [EmbeddedSkill],
    ) -> Result<Self, ServerSkillsError> {
        if server.as_str() == HUB_AUTHORITY {
            return Err(ServerSkillsError::ReservedAuthority(server));
        }
        let mut skills = Vec::with_capacity(table.len());
        let mut files = BTreeMap::new();
        for embedded in table {
            let invalid = |reason: String| ServerSkillsError::Invalid {
                skill: embedded.name.to_owned(),
                reason,
            };
            let frontmatter: SkillFrontmatter = serde_json::from_str(embedded.frontmatter)
                .map_err(|error| invalid(format!("frontmatter: {error}")))?;
            let mut resources = Vec::with_capacity(embedded.files.len());
            for file in embedded.files {
                let uri = SkillUri::of(&server, embedded.name, file.path)
                    .map_err(|error| invalid(format!("{}: {error}", file.path)))?;
                resources.push(SkillResource::new(uri.as_str(), file.digest, file.size));
                files.insert(uri.as_str().to_owned(), file);
            }
            let skill_md = SkillUri::of(&server, embedded.name, SKILL_MD)
                .map_err(|error| invalid(error.to_string()))?;
            let skill = Skill::new(
                skill_md.as_str(),
                frontmatter,
                SkillResources::Files(resources),
            );
            skill
                .validate()
                .map_err(|error| invalid(error.to_string()))?;
            skills.push(skill);
        }
        Ok(Self {
            server: Some(server),
            skills,
            files,
        })
    }

    /// The server whose skills these are; `None` for a server without skills.
    pub fn server(&self) -> Option<&ServerSlug> {
        self.server.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Every skill's entry, in name order.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// The address of every skill file, in address order.
    pub fn file_uris(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// `skills/list`: every skill, in one page.
    pub fn list(&self) -> ListSkillsResult {
        ListSkillsResult::with_all_items(self.skills.clone())
    }

    /// `skills/get`: the skill whose `SKILL.md` is at `uri`, or `-32602` when this server
    /// serves no skill there.
    pub fn get(&self, uri: &str) -> Result<GetSkillResult, ErrorData> {
        self.skills
            .iter()
            .find(|skill| skill.uri == uri)
            .map(|skill| GetSkillResult::new(skill.clone()))
            .ok_or_else(|| ErrorData::invalid_params("no skill at this address", None))
    }

    /// Whether `uri` addresses one of this server's skill files, or would if it existed:
    /// the reads [`Self::read`] answers.
    pub fn addresses(&self, uri: &str) -> bool {
        self.server
            .as_ref()
            .is_some_and(|server| super::owned_by(uri, server))
    }

    /// `resources/read` of a skill file: its text, or its bytes in base64 when they are not
    /// UTF-8, with a media type from its extension. `None` for any address that is not
    /// this server's skill file; a missing file of its skills is not found.
    pub fn read(
        &self,
        request: &ReadResourceRequestParams,
    ) -> Result<Option<ReadResourceResponse>, ErrorData> {
        if !self.addresses(&request.uri) {
            return Ok(None);
        }
        if request.request_state.is_some() || request.input_responses.is_some() {
            return Err(ErrorData::invalid_params(
                "skill reads do not accept continuation state",
                None,
            ));
        }
        let file = self
            .files
            .get(&request.uri)
            .ok_or_else(|| ErrorData::resource_not_found("no skill file at this address", None))?;
        let media_type = media_type(file.path);
        let contents = match std::str::from_utf8(file.bytes) {
            Ok(text) => ResourceContents::text(text, &request.uri).with_mime_type(media_type),
            Err(_) => ResourceContents::blob(
                base64::engine::general_purpose::STANDARD.encode(file.bytes),
                &request.uri,
            )
            .with_mime_type("application/octet-stream"),
        };
        Ok(Some(ReadResourceResult::new(vec![contents]).into()))
    }
}

/// The media type of a text file of a skill, by its extension.
fn media_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("md") => "text/markdown",
        Some("json") => "application/json",
        Some("yaml" | "yml") => "application/yaml",
        Some("html") => "text/html",
        Some("csv") => "text/csv",
        _ => "text/plain",
    }
}

/// The skills embedded from the calling crate's `skills/` directory, for its
/// [`McpServerContract::skills`](crate::server_contract::McpServerContract::skills). The
/// crate's build script runs `veoveo_skills::build::embed_skills("skills")`, and the crate
/// depends on `veoveo-skills`, whose types the embedded table names.
#[macro_export]
macro_rules! server_skills {
    ($server:expr) => {
        $crate::skills::ServerSkills::embedded(
            ::veoveo_types::ServerSlug::parse($server).expect("declared server identity"),
            include!(concat!(env!("OUT_DIR"), "/veoveo_skills.rs")),
        )
        .expect("the embedded skills")
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    static FILES: [EmbeddedFile; 2] = [
        EmbeddedFile {
            path: "SKILL.md",
            bytes: b"---\nname: watch\ndescription: Watch builds.\n---\n",
            digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            size: 46,
        },
        EmbeddedFile {
            path: "logo.bin",
            bytes: &[0xff, 0xfe],
            digest: "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            size: 2,
        },
    ];
    static TABLE: [EmbeddedSkill; 1] = [EmbeddedSkill {
        name: "watch",
        frontmatter: r#"{"name":"watch","description":"Watch builds.","metadata":{"audience":"agents"}}"#,
        files: &FILES,
    }];

    fn skills() -> ServerSkills {
        ServerSkills::embedded(ServerSlug::parse("console").unwrap(), &TABLE).unwrap()
    }

    fn read(uri: &str) -> Result<Option<ReadResourceResponse>, ErrorData> {
        skills().read(&ReadResourceRequestParams::new(uri))
    }

    fn contents(uri: &str) -> serde_json::Value {
        let Some(ReadResourceResponse::Complete(result)) = read(uri).unwrap() else {
            panic!("a complete read of {uri}")
        };
        serde_json::to_value(result).unwrap()["contents"][0].clone()
    }

    #[test]
    fn skills_are_listed_and_got_with_their_manifest() {
        let skills = skills();
        let listed = skills.list();
        assert_eq!(listed.skills.len(), 1);
        let skill = &listed.skills[0];
        assert_eq!(skill.uri, "skill://console/watch/SKILL.md");
        assert_eq!(skill.frontmatter.extra["metadata"]["audience"], "agents");
        let SkillResources::Files(files) = &skill.resources else {
            panic!("a full manifest")
        };
        assert_eq!(
            files
                .iter()
                .map(|file| (file.uri.as_str(), file.size))
                .collect::<Vec<_>>(),
            [
                ("skill://console/watch/SKILL.md", 46),
                ("skill://console/watch/logo.bin", 2)
            ]
        );
        assert_eq!(
            skills.get("skill://console/watch/SKILL.md").unwrap().skill,
            *skill
        );
        assert!(skills.get("skill://console/other/SKILL.md").is_err());
    }

    #[test]
    fn skill_files_read_as_text_or_base64() {
        let text = contents("skill://console/watch/SKILL.md");
        assert_eq!(text["mimeType"], "text/markdown");
        assert!(text["text"].as_str().unwrap().starts_with("---"));
        assert_eq!(contents("skill://console/watch/logo.bin")["blob"], "//4=");
        // Another server's skill or another scheme is the domain's to read; a missing file
        // of this server's skills is not found.
        assert!(read("skill://time/watch/SKILL.md").unwrap().is_none());
        assert!(read("console://docs/agents").unwrap().is_none());
        assert!(read("skill://console/watch/missing.md").is_err());
    }

    #[test]
    fn the_hubs_authority_holds_no_servers_skills() {
        if let Ok(veoveo) = ServerSlug::parse(HUB_AUTHORITY) {
            assert!(matches!(
                ServerSkills::embedded(veoveo, &TABLE),
                Err(ServerSkillsError::ReservedAuthority(_))
            ));
        }
        assert!(ServerSkills::none().is_empty());
    }
}
