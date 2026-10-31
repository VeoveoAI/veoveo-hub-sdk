//! The public base URL of an installation and the endpoint each hosted server mounts under it.

use std::{fmt, str::FromStr};

use anyhow::{Result, anyhow};
use url::Url;

use crate::{ServerSlug, registration::PublicBase};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicDeployment {
    base_url: String,
    host_authority: String,
    origin: Option<PublicOrigin>,
    public_base: Option<PublicBase>,
}

/// The installation's public origin, `VEOVEO_PUBLIC_ORIGIN`: the one HTTPS origin an
/// installation serves to the internet, such as `https://veoveo.example.com`, under which
/// each server's public routes lie at its own path. A server admits the origin's host
/// beside its deployment host and builds the public URLs it hands out under the origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrigin {
    url: Url,
    host_authority: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPublicEndpoint {
    public_url: String,
    mount_path: String,
}

impl PublicDeployment {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self> {
        let (base_url, host_authority) = normalize_base_url(base_url.as_ref())?;
        Ok(Self {
            base_url,
            host_authority,
            origin: None,
            public_base: None,
        })
    }

    /// The deployment under the installation's public origin, when it has one. Requests
    /// for the origin's host pass host validation beside the deployment host's.
    pub fn with_origin(mut self, origin: Option<PublicOrigin>) -> Self {
        self.origin = origin;
        self
    }

    /// The installation's public origin, when it has one.
    pub fn origin(&self) -> Option<&PublicOrigin> {
        self.origin.as_ref()
    }

    /// The deployment under the public URL of this instance's mount, the
    /// `veoveo_public_base` of its registration response, when it names one. Requests for
    /// the base's host pass host validation beside the deployment host's, and
    /// [`public_url`](Self::public_url) builds every public URL under the base, path
    /// included. A server passes its base here instead of the installation's origin.
    pub fn with_public_base(mut self, base: Option<PublicBase>) -> Self {
        self.public_base = base;
        self
    }

    /// The public URL of this instance's mount, when the deployment has one.
    pub fn public_base(&self) -> Option<&PublicBase> {
        self.public_base.as_ref()
    }

    /// The public URL of `segments` under `server`'s mount, each segment percent-encoded:
    /// under the public base when the deployment has one, otherwise under
    /// `{origin}/{server}` when it has the installation's origin, and `None` without
    /// either.
    pub fn public_url<'a>(
        &self,
        server: &ServerSlug,
        segments: impl IntoIterator<Item = &'a str>,
    ) -> Option<Url> {
        match (&self.public_base, &self.origin) {
            (Some(base), _) => Some(base.join(segments)),
            (None, Some(origin)) => {
                let mut url = origin.join([server.as_str()]);
                url.path_segments_mut()
                    .expect("an https URL has a path")
                    .extend(segments);
                Some(url)
            }
            (None, None) => None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn host_authority(&self) -> &str {
        &self.host_authority
    }

    pub fn server(&self, server_slug: impl AsRef<str>) -> Result<ServerPublicEndpoint> {
        let server_slug = normalize_server_slug(server_slug.as_ref())?;
        let mount_path = format!("/{server_slug}");
        let public_url = format!("{}{}", self.base_url, mount_path);
        Ok(ServerPublicEndpoint {
            public_url,
            mount_path,
        })
    }
}

impl ServerPublicEndpoint {
    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub fn mount_path(&self) -> &str {
        &self.mount_path
    }

    pub fn path(&self, child: &str) -> String {
        let child = child.trim_matches('/');
        if child.is_empty() {
            self.mount_path.clone()
        } else {
            format!("{}/{}", self.mount_path, child)
        }
    }

    pub fn url(&self, child: &str) -> String {
        let child = child.trim_matches('/');
        if child.is_empty() {
            self.public_url.clone()
        } else {
            format!("{}/{}", self.public_url, child)
        }
    }
}

impl PublicOrigin {
    /// Admit an origin: an `https` URL with a host, and no user information, path, query
    /// or fragment.
    pub fn parse(input: &str) -> Result<Self> {
        let url = Url::parse(input.trim())
            .map_err(|error| anyhow!("VEOVEO_PUBLIC_ORIGIN is no URL: {error}"))?;
        if url.scheme() != "https" {
            return Err(anyhow!("VEOVEO_PUBLIC_ORIGIN must be an https URL"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(anyhow!("VEOVEO_PUBLIC_ORIGIN must not include userinfo"));
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(anyhow!(
                "VEOVEO_PUBLIC_ORIGIN must be an origin, without a path, query or fragment"
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("VEOVEO_PUBLIC_ORIGIN must include a host"))?;
        let host_authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        Ok(Self {
            url,
            host_authority,
        })
    }

    /// The origin as a URL with the root path, such as `https://veoveo.example.com/`.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The host and port requests for the origin name, such as `veoveo.example.com`.
    pub fn host_authority(&self) -> &str {
        &self.host_authority
    }

    /// The URL of `segments` under the origin, each segment percent-encoded.
    pub fn join<'a>(&self, segments: impl IntoIterator<Item = &'a str>) -> Url {
        let mut url = self.url.clone();
        url.path_segments_mut()
            .expect("an https URL has a path")
            .pop_if_empty()
            .extend(segments);
        url
    }
}

impl FromStr for PublicOrigin {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        Self::parse(input)
    }
}

impl fmt::Display for PublicOrigin {
    /// The origin without the root path, such as `https://veoveo.example.com`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.url.as_str().trim_end_matches('/'))
    }
}

fn normalize_base_url(input: &str) -> Result<(String, String)> {
    let value = input.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return Err(anyhow!("missing PUBLIC_BASE_URL"));
    }
    let authority = if let Some(rest) = value.strip_prefix("http://") {
        rest
    } else if let Some(rest) = value.strip_prefix("https://") {
        rest
    } else {
        return Err(anyhow!(
            "PUBLIC_BASE_URL must start with http:// or https://"
        ));
    };
    if value.contains(['?', '#']) || value.chars().any(char::is_whitespace) {
        return Err(anyhow!(
            "PUBLIC_BASE_URL must not contain whitespace, query, or fragment"
        ));
    }
    if authority.is_empty() {
        return Err(anyhow!("PUBLIC_BASE_URL must include a host"));
    }
    if authority.contains('/') {
        return Err(anyhow!(
            "PUBLIC_BASE_URL must be an origin and must not include a path"
        ));
    }
    if authority.contains('@') {
        return Err(anyhow!("PUBLIC_BASE_URL must not include userinfo"));
    }
    let host_authority = authority.to_string();
    Ok((value, host_authority))
}

fn normalize_server_slug(input: &str) -> Result<String> {
    let value = input.trim();
    validate_path_segment(value, "server slug")?;
    ServerSlug::parse(value)?;
    Ok(value.to_string())
}

fn validate_path_segment(value: &str, name: &str) -> Result<()> {
    if value.is_empty() {
        return Err(anyhow!("{name} must not be empty"));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(anyhow!(
            "{name} must contain only lowercase ASCII letters, digits, hyphen, or underscore"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_server_public_endpoint_for_operator_owned_domain() {
        let deployment =
            PublicDeployment::new("https://veoveo.enterprise.example/").expect("valid deployment");
        let media = deployment.server("media").expect("valid server");

        assert_eq!(deployment.base_url(), "https://veoveo.enterprise.example");
        assert_eq!(deployment.host_authority(), "veoveo.enterprise.example");
        assert_eq!(media.mount_path(), "/media");
        assert_eq!(
            media.public_url(),
            "https://veoveo.enterprise.example/media"
        );
        assert_eq!(media.path("mcp"), "/media/mcp");
    }

    #[test]
    fn the_public_origin_is_one_https_origin() {
        let origin = PublicOrigin::parse("https://Veoveo.Example.com/").expect("an origin");
        assert_eq!(origin.host_authority(), "veoveo.example.com");
        assert_eq!(origin.to_string(), "https://veoveo.example.com");
        assert_eq!(
            origin.join(["artifact", "shares", ""]).as_str(),
            "https://veoveo.example.com/artifact/shares/"
        );
        let ported = PublicOrigin::parse("https://veoveo.example.com:8443").expect("an origin");
        assert_eq!(ported.host_authority(), "veoveo.example.com:8443");
        for refused in [
            "http://veoveo.example.com",
            "https://veoveo.example.com/base",
            "https://veoveo.example.com/?x=1",
            "https://veoveo.example.com/#top",
            "https://user@veoveo.example.com",
            "veoveo.example.com",
        ] {
            assert!(
                PublicOrigin::parse(refused).is_err(),
                "{refused} was admitted"
            );
        }
        let deployment = PublicDeployment::new("http://media.internal:8802")
            .expect("valid deployment")
            .with_origin(Some(origin.clone()));
        assert_eq!(deployment.origin(), Some(&origin));
    }

    #[test]
    fn public_urls_lie_under_the_public_base_or_the_origin() {
        let media = ServerSlug::parse("media").unwrap();
        let deployment = PublicDeployment::new("http://media.internal:8802").unwrap();
        assert_eq!(deployment.public_url(&media, ["shares"]), None);
        let deployment = deployment.with_origin(Some(
            PublicOrigin::parse("https://veoveo.example.com").unwrap(),
        ));
        assert_eq!(
            deployment
                .public_url(&media, ["shares", "a b"])
                .unwrap()
                .as_str(),
            "https://veoveo.example.com/media/shares/a%20b"
        );
        let base = PublicBase::parse("https://instance-7.veoveo.example.com/b/7/media").unwrap();
        let deployment = deployment.with_public_base(Some(base.clone()));
        assert_eq!(deployment.public_base(), Some(&base));
        assert_eq!(
            deployment
                .public_url(&media, ["shares", "t"])
                .unwrap()
                .as_str(),
            "https://instance-7.veoveo.example.com/b/7/media/shares/t"
        );
    }

    #[test]
    fn rejects_base_url_paths() {
        let error = PublicDeployment::new("https://veoveo.enterprise.example/base")
            .expect_err("base URL path should fail");
        assert!(error.to_string().contains("must not include a path"));
    }
}
