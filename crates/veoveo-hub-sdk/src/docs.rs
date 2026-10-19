//! Embedded server documents and the contract self-declaration.
//!
//! Implements the Well-Known Surface of `mcp/contract/DESIGN.md` (C18-C21):
//! documents embedded at build time from the server crate, the
//! machine-readable contract declaration served at `{scheme}://contract`, and
//! llms.txt rendering for the administrative mount. Servers obtain the
//! document set with the [`server_docs!`](crate::server_docs) macro so the
//! deployed binary serves the manual of exactly the version it was built
//! from.

use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::Serialize;

pub mod catalog;
mod declaration;
#[cfg(feature = "server")]
mod served;
pub use catalog::{RequirementCatalogFormat, RequirementId};
pub use declaration::ContractDeclaration;
use sha2::{Digest as _, Sha256};
#[doc(hidden)]
pub use veoveo_types::embedded_document;
use veoveo_types::{
    ResourceScheme, ResourceTemplateUri, ResourceUri, ResourceUriBuilder, Sha256Digest,
    UriAuthority, UriSegment, identifier_syntax::PathIdProfile,
};

/// The hosted MCP server contract this crate defines.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, veoveo_types::Vocabulary)]
#[schemars(transform = veoveo_types::naming::contract_uri_naming)]
pub enum ContractId {
    #[vocabulary(rename = "https://veoveo.ai/contracts/hosted-mcp")]
    HostedMcp,
}

/// Identifier of the required agent manual document.
pub const DOC_ID_AGENTS: &str = "agents";

/// Identifier of the required domain design document.
pub const DOC_ID_DESIGN: &str = "design";

/// Section headers every server `AGENTS.md` must contain (C23).
pub const REQUIRED_AGENT_SECTIONS: [&str; 3] = ["## Purpose", "## Invariants", "## Build And Test"];

/// One document embedded from the server crate at build time.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ServerDoc {
    pub id: &'static str,
    pub title: &'static str,
    #[serde(skip)]
    pub body: &'static str,
    #[serde(skip)]
    pub digest: Sha256Digest,
}

/// The embedded document set a server serves under `{scheme}://docs`.
#[derive(Debug, Clone)]
pub struct ServerDocs {
    server: veoveo_types::ServerSlug,
    docs: Vec<ServerDoc>,
    declaration: OnceLock<ContractDeclaration>,
}

impl ServerDocs {
    pub fn new(server: veoveo_types::ServerSlug) -> Self {
        Self {
            server,
            docs: Vec::new(),
            declaration: OnceLock::new(),
        }
    }

    pub fn with_doc(mut self, id: &'static str, title: &'static str, body: &'static str) -> Self {
        self.docs.push(ServerDoc {
            id,
            title,
            body,
            digest: Sha256Digest::from_bytes(Sha256::digest(body.as_bytes()).into()),
        });
        self
    }

    /// Production documents use a digest emitted by the compile-time macro.
    pub fn with_embedded_doc(
        mut self,
        id: &'static str,
        title: &'static str,
        embedded: (&'static str, [u8; 32]),
    ) -> Self {
        self.docs.push(ServerDoc {
            id,
            title,
            body: embedded.0,
            digest: Sha256Digest::from_bytes(embedded.1),
        });
        self
    }

    pub fn identity(&self) -> &veoveo_types::ServerSlug {
        &self.server
    }

    pub fn server(&self) -> &str {
        self.server.as_str()
    }

    pub fn doc(&self, id: &str) -> Option<&ServerDoc> {
        self.docs.iter().find(|doc| doc.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ServerDoc> {
        self.docs.iter()
    }

    /// The llms.txt index served at `{mount}/admin/docs/llms.txt` (C20).
    pub fn llms_txt(&self) -> String {
        let mut out = format!(
            "# {}\n\n> Veoveo MCP server documents under the {} contract.\n\n## Docs\n\n",
            self.server,
            ContractId::HostedMcp.as_str()
        );
        for doc in &self.docs {
            out.push_str(&format!("- [{}]({})\n", doc.title, doc.id));
        }
        out
    }

    /// The agent manual embedded from the crate `AGENTS.md`, when present.
    pub fn agent_manual(&self) -> Option<&'static str> {
        self.doc(DOC_ID_AGENTS).map(|doc| doc.body)
    }

    /// Returns the declaration built once from this document set.
    ///
    /// Discover and the list methods are the only authority for the observed
    /// running protocol surface. The embedded contract resource declares only
    /// the contract and provider identity.
    pub fn contract_declaration(&self) -> &ContractDeclaration {
        self.declaration
            .get_or_init(|| ContractDeclaration::from_docs(self))
    }
}

/// The identifier of one served document, such as `agents` or `design`.
#[veoveo_types::id(text(PathIdProfile))]
pub struct DocumentId(String);

/// Documents per page of the `{scheme}://docs` index.
pub const DOC_PAGE_SIZE: usize = 32;

/// The index of a server's documents, `{scheme}://docs`.
pub fn docs_index_uri(scheme: &ResourceScheme) -> ResourceUri {
    ResourceUriBuilder::from_components(
        scheme,
        UriAuthority::new("docs").expect("declared authority"),
    )
    .expect("resource scheme")
    .build()
    .expect("docs URI")
}

/// One document, `{scheme}://docs/{doc_id}`.
pub fn docs_member_uri(scheme: &ResourceScheme, id: &DocumentId) -> ResourceUri {
    ResourceUriBuilder::new(docs_index_uri(scheme).as_str())
        .expect("docs URI")
        .segment(UriSegment::new(id.as_str()).expect("validated document id"))
        .build()
        .expect("docs member URI")
}

/// The template every server declares for its documents.
pub fn docs_member_template(scheme: &ResourceScheme) -> ResourceTemplateUri {
    // The variable is a fixed route declaration; concrete IDs always use the URI builder.
    ResourceTemplateUri::new(format!("{}/{{doc_id}}", docs_index_uri(scheme)))
        .expect("declared docs template")
}

/// One entry of the documents index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentEntry {
    pub id: DocumentId,
    pub title: String,
    pub uri: ResourceUri,
}

/// One page of the documents index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentPage {
    pub items: Vec<DocumentEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<DocumentId>,
}

/// Why a documents page could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DocumentPageError {
    #[error("duplicate document id")]
    Duplicate,
    #[error("unknown document cursor")]
    UnknownCursor,
}

/// Pages an immutable document set. A cursor is the last returned document ID; sorted
/// keyset paging never returns it again and rejects IDs absent from the set.
pub fn page_documents(
    mut entries: Vec<DocumentEntry>,
    after: Option<&DocumentId>,
) -> Result<DocumentPage, DocumentPageError> {
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    if entries.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(DocumentPageError::Duplicate);
    }
    let start = match after {
        Some(id) => {
            entries
                .binary_search_by(|v| v.id.cmp(id))
                .map_err(|_| DocumentPageError::UnknownCursor)?
                + 1
        }
        None => 0,
    };
    let end = (start + DOC_PAGE_SIZE).min(entries.len());
    let next_cursor = (end < entries.len()).then(|| entries[end - 1].id.clone());
    Ok(DocumentPage {
        items: entries.drain(start..end).collect(),
        next_cursor,
    })
}

/// Embeds the crate's `AGENTS.md` and `DESIGN.md` as its served document set
/// (C18, C21). Invoke from the server crate so the paths resolve against that
/// crate's manifest directory.
#[macro_export]
macro_rules! server_docs {
    ($server:expr) => {
        $crate::docs::ServerDocs::new(
            ::veoveo_types::ServerSlug::parse($server).expect("declared server identity"),
        )
        .with_embedded_doc(
            $crate::docs::DOC_ID_AGENTS,
            "Agent work manual",
            $crate::docs::embedded_document!("AGENTS.md"),
        )
        .with_embedded_doc(
            $crate::docs::DOC_ID_DESIGN,
            "Domain design",
            $crate::docs::embedded_document!("DESIGN.md"),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llms_txt_lists_every_document() {
        let docs = ServerDocs::new(veoveo_types::ServerSlug::parse("example").unwrap())
            .with_doc(DOC_ID_AGENTS, "Agent work manual", "body")
            .with_doc(DOC_ID_DESIGN, "Domain design", "body");
        let index = docs.llms_txt();
        assert!(index.starts_with("# example\n"));
        assert!(index.contains("- [Agent work manual](agents)"));
        assert!(index.contains("- [Domain design](design)"));
        assert!(index.contains(ContractId::HostedMcp.as_str()));
        assert_eq!(ContractId::HostedMcp.as_str(), crate::HOSTED_MCP_CONTRACT);
    }
}
