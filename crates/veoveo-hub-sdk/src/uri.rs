use veoveo_types::ResourceScheme;

/// The resources every hosted server serves under its own scheme: its embedded
/// documents and its contract declaration.
#[derive(Debug, Clone)]
pub struct ServerResourceUris {
    scheme: ResourceScheme,
}

impl ServerResourceUris {
    pub fn new(scheme: ResourceScheme) -> Self {
        Self { scheme }
    }

    pub fn scheme(&self) -> &ResourceScheme {
        &self.scheme
    }

    /// Index of the server's embedded documents (contract C18).
    pub fn docs_root_uri(&self) -> String {
        format!("{}://docs", self.scheme)
    }

    pub fn doc_uri(&self, doc_id: &str) -> String {
        format!("{}://docs/{}", self.scheme, doc_id)
    }

    pub fn doc_template(&self) -> String {
        format!("{}://docs/{{doc_id}}", self.scheme)
    }

    /// Parses one document body URI under the server's canonical docs root.
    pub fn parse_doc_uri<'a>(&self, uri: &'a str) -> Option<&'a str> {
        parse_server_doc_uri(self.scheme.as_str(), uri)
    }

    /// The machine-readable contract declaration (contract C19).
    pub fn contract_uri(&self) -> veoveo_types::ResourceUri {
        veoveo_types::ResourceUriBuilder::from_components(
            &self.scheme,
            veoveo_types::UriAuthority::new("contract").expect("fixed authority"),
        )
        .expect("validated scheme")
        .build()
        .expect("contract URI")
    }
}

/// Parses `{scheme}://docs/{doc_id}` using the shared one-segment contract.
pub fn parse_server_doc_uri<'a>(scheme: &str, uri: &'a str) -> Option<&'a str> {
    uri.strip_prefix(&format!("{scheme}://docs/"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_resource_uri_conventions_round_trip() {
        let uris = ServerResourceUris::new(ResourceScheme::parse("example").unwrap());
        assert_eq!(uris.docs_root_uri(), "example://docs");
        assert_eq!(uris.doc_uri("agents"), "example://docs/agents");
        assert_eq!(uris.parse_doc_uri("example://docs/agents"), Some("agents"));
        assert_eq!(uris.parse_doc_uri("example://docs"), None);
        assert_eq!(uris.parse_doc_uri("example://docs/agents/extra"), None);
        assert_eq!(uris.contract_uri().as_str(), "example://contract");
    }
}
