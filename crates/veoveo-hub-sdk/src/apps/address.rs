//! The address of an MCP App: `ui://{slug}/{page}`.

use std::fmt;

use veoveo_types::{ResourceAddress, ResourceUri, ResourceUriParts, ServerSlug};

/// An App's `ui://{slug}/{page}` address: the `ui` scheme, the owning server's slug as
/// the authority and a page path of one or more nonempty segments, in canonical form,
/// without query or fragment. The slug names the server whose App it is; hosts derive
/// ownership from it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppUri {
    uri: ResourceUri,
    server: ServerSlug,
}

/// Why a value is not an App address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an App address is ui://{{slug}}/{{page}} in canonical form: {0}")]
pub struct AppUriError(&'static str);

impl AppUri {
    pub fn parse(value: &str) -> Result<Self, AppUriError> {
        let parts =
            ResourceUriParts::parse(value).map_err(|_| AppUriError("not a canonical URI"))?;
        if parts.scheme() != "ui" {
            return Err(AppUriError("the scheme is ui"));
        }
        let server = ServerSlug::parse(parts.authority())
            .map_err(|_| AppUriError("the authority is a server slug"))?;
        let segments: Vec<_> = parts.path_segments().collect();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            return Err(AppUriError(
                "the page is one or more nonempty path segments",
            ));
        }
        if parts.has_query() {
            return Err(AppUriError("an App address carries no query"));
        }
        let uri = ResourceUri::new(value).map_err(|_| AppUriError("not a resource URI"))?;
        Ok(Self { uri, server })
    }

    /// The server whose App this is.
    pub fn server(&self) -> &ServerSlug {
        &self.server
    }

    pub fn as_str(&self) -> &str {
        self.uri.as_str()
    }

    pub fn resource_uri(&self) -> &ResourceUri {
        &self.uri
    }
}

impl fmt::Display for AppUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ResourceAddress for AppUri {
    type Error = AppUriError;

    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        Self::parse(uri.as_str())
    }

    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        Ok(self.uri.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_address_names_its_server_and_a_page() {
        let app = AppUri::parse("ui://time/now").unwrap();
        assert_eq!(app.server().as_str(), "time");
        assert_eq!(app.as_str(), "ui://time/now");
        assert!(AppUri::parse("ui://timeseries/forecast.html").is_ok());
        assert!(AppUri::parse("ui://media/views/create").is_ok());
        for refused in [
            "ui://time",
            "ui://time/",
            "ui://time//now",
            "ui://time/now/",
            "ui://time/now?x=1",
            "ui://time/now#top",
            "ui://Time/now",
            "time://time/now",
            "ui:time/now",
            "ui://time/a/../now",
        ] {
            assert!(AppUri::parse(refused).is_err(), "{refused}");
        }
    }
}
