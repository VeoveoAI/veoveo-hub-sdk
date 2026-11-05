//! `veoveo_surfaces`: the HTTP a server serves besides MCP, which it declares so that the
//! Hub can route it. Public routes carry no Hub authentication and serve only callers
//! that verify themselves; the app origin carries a full web application on the
//! server's own origin; a viewer is a page under a public route that opens files of the
//! extensions it takes on the content domain. MCP Discover advertises the same declaration
//! as `surfaces` in `ai.veoveo/registration`.
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::ServerSlug;

use crate::registration::DeclarationError;

/// The largest request body a public route may admit: 16 MiB.
pub const MAX_PUBLIC_BODY_BYTES: u64 = 16 * 1024 * 1024;
/// The highest request rate a public route may declare: 6000 requests a minute.
pub const MAX_PUBLIC_REQUESTS_PER_MINUTE: u32 = 6000;

/// What a server serves besides MCP. A server with no public route, app origin or viewer
/// omits the declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfacesDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_routes: Vec<PublicRouteDeclaration>,
    /// Whether the server serves its `authenticated_routes` on its own app origin,
    /// behind the `app_origin` requirement of its authorization declaration.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub app_origin: bool,
    /// The viewer the server offers for files on the content domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<ViewerDeclaration>,
}

/// A viewer: the Hub opens a file of one of `extensions` on the content domain with the
/// page this server serves at `route`, one of its public routes, handing it the file's
/// path in the query parameter `file`. The page reads the file from its own origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewerDeclaration {
    pub extensions: Vec<FileExtension>,
    pub route: PublicRoutePrefix,
}

/// One public route prefix with the largest request body it admits and its rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicRouteDeclaration {
    pub prefix: PublicRoutePrefix,
    pub max_body_bytes: BodyLimit,
    pub requests_per_minute: RequestRate,
}

impl SurfacesDeclaration {
    /// Whether the declaration declares nothing, so the member is omitted.
    pub fn is_empty(&self) -> bool {
        self.public_routes.is_empty() && !self.app_origin && self.viewer.is_none()
    }

    /// Check the declaration of `server`, whose authorization declares the `app_origin`
    /// requirement when `app_origin_requirement` holds: every prefix lies under the
    /// mount `/{server}`, covers neither `{mount}/mcp` nor `{mount}/admin` and overlaps
    /// no other, an app origin is declared exactly with its requirement, and a viewer takes
    /// at least one extension, each once, at one of the public routes.
    pub fn check(
        &self,
        server: &ServerSlug,
        app_origin_requirement: bool,
    ) -> Result<(), DeclarationError> {
        if self.is_empty() {
            return Err(DeclarationError::invalid(
                "veoveo_surfaces",
                "a server with no public route, app origin or viewer omits the member",
            ));
        }
        for (index, route) in self.public_routes.iter().enumerate() {
            let Some(within) = route.prefix.within_mount(server) else {
                return Err(DeclarationError::PublicRouteOutsideMount {
                    prefix: route.prefix.as_str().to_owned(),
                });
            };
            if ["/mcp/", "/admin/"]
                .iter()
                .any(|reserved| within.starts_with(reserved))
            {
                return Err(DeclarationError::PublicRouteReserved {
                    prefix: route.prefix.as_str().to_owned(),
                });
            }
            if let Some(other) = self.public_routes[..index]
                .iter()
                .find(|other| other.prefix.overlaps(&route.prefix))
            {
                return Err(DeclarationError::PublicRouteOverlap {
                    prefix: route.prefix.as_str().to_owned(),
                    other: other.prefix.as_str().to_owned(),
                });
            }
        }
        if self.app_origin != app_origin_requirement {
            return Err(DeclarationError::invalid(
                "app_origin",
                "a server declares the app_origin requirement exactly when it declares an app origin",
            ));
        }
        if let Some(viewer) = &self.viewer {
            if viewer.extensions.is_empty() {
                return Err(DeclarationError::invalid(
                    "viewer",
                    "a viewer takes at least one extension",
                ));
            }
            let distinct = viewer
                .extensions
                .iter()
                .collect::<std::collections::BTreeSet<_>>();
            if distinct.len() != viewer.extensions.len() {
                return Err(DeclarationError::invalid(
                    "viewer",
                    "a viewer names each extension once",
                ));
            }
            if !self
                .public_routes
                .iter()
                .any(|route| route.prefix == viewer.route)
            {
                return Err(DeclarationError::invalid(
                    "viewer",
                    "a viewer's route is one of the server's public routes",
                ));
            }
        }
        Ok(())
    }
}

/// A file extension a viewer takes: 1 to 32 lowercase ASCII letters and digits, without its
/// dot, such as `usdz`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
#[schemars(extend("pattern" = r"^[a-z0-9]{1,32}$"))]
pub struct FileExtension(String);

impl FileExtension {
    pub fn new(value: impl Into<String>) -> Result<Self, DeclarationError> {
        let value = value.into();
        if !(1..=32).contains(&value.len())
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(DeclarationError::invalid(
                "extensions",
                "an extension is 1 to 32 lowercase ASCII letters and digits, without its dot",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for FileExtension {
    type Error = DeclarationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<FileExtension> for String {
    fn from(value: FileExtension) -> Self {
        value.0
    }
}

/// A public route prefix: an absolute path of nonempty segments of RFC 3986 unreserved
/// characters, ending with `/`, such as `/ledger/hooks/`. Its first segment is the
/// server's mount.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
#[schemars(extend("pattern" = r"^(/[A-Za-z0-9._~-]+)+/$"))]
pub struct PublicRoutePrefix(String);

impl PublicRoutePrefix {
    pub fn new(value: impl Into<String>) -> Result<Self, DeclarationError> {
        let value = value.into();
        let segments = value
            .strip_prefix('/')
            .and_then(|path| path.strip_suffix('/'))
            .map(|path| path.split('/'));
        let admitted = segments.is_some_and(|mut segments| {
            segments.all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            })
        });
        if !admitted {
            return Err(DeclarationError::invalid(
                "prefix",
                "an absolute path of nonempty unreserved segments ending with /",
            ));
        }
        Ok(Self(value))
    }

    /// The prefix `{mount}/{path}` of `server`'s mount, where `path` is relative to
    /// the mount and ends with `/`, such as `hooks/`.
    pub fn under(server: &ServerSlug, path: &str) -> Result<Self, DeclarationError> {
        Self::new(format!("/{}/{path}", server.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The prefix relative to `server`'s mount, such as `/hooks/`, when it lies under
    /// the mount below the mount itself.
    pub fn within_mount(&self, server: &ServerSlug) -> Option<&str> {
        self.0
            .strip_prefix('/')
            .and_then(|path| path.strip_prefix(server.as_str()))
            .filter(|within| within.starts_with('/') && within.len() > 1)
    }

    /// Whether either prefix covers the other.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }
}
impl TryFrom<String> for PublicRoutePrefix {
    type Error = DeclarationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<PublicRoutePrefix> for String {
    fn from(value: PublicRoutePrefix) -> Self {
        value.0
    }
}
impl fmt::Display for PublicRoutePrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The largest request body a public route admits, 1 byte to 16 MiB.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "u64", into = "u64")]
#[schemars(extend("minimum" = 1, "maximum" = MAX_PUBLIC_BODY_BYTES))]
pub struct BodyLimit(u64);

impl BodyLimit {
    pub fn new(bytes: u64) -> Result<Self, DeclarationError> {
        if !(1..=MAX_PUBLIC_BODY_BYTES).contains(&bytes) {
            return Err(DeclarationError::invalid(
                "max_body_bytes",
                "a body limit is 1 byte to 16 MiB",
            ));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for BodyLimit {
    type Error = DeclarationError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<BodyLimit> for u64 {
    fn from(value: BodyLimit) -> Self {
        value.0
    }
}

/// A public route's request rate, 1 to 6000 requests a minute.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "u32", into = "u32")]
#[schemars(extend("minimum" = 1, "maximum" = MAX_PUBLIC_REQUESTS_PER_MINUTE))]
pub struct RequestRate(u32);

impl RequestRate {
    pub fn per_minute(requests: u32) -> Result<Self, DeclarationError> {
        if !(1..=MAX_PUBLIC_REQUESTS_PER_MINUTE).contains(&requests) {
            return Err(DeclarationError::invalid(
                "requests_per_minute",
                "a rate is 1 to 6000 requests a minute",
            ));
        }
        Ok(Self(requests))
    }
    pub fn requests_per_minute(self) -> u32 {
        self.0
    }
}
impl TryFrom<u32> for RequestRate {
    type Error = DeclarationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::per_minute(value)
    }
}
impl From<RequestRate> for u32 {
    fn from(value: RequestRate) -> Self {
        value.0
    }
}

/// [`SurfacesDeclaration`] as the `veoveo_surfaces` registration member spells it: OAuth
/// documents use snake_case member names.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SurfacesDocument")]
pub(crate) struct SurfacesDocument {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    public_routes: Vec<PublicRouteDocument>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    app_origin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    viewer: Option<ViewerDocument>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ViewerDocument")]
struct ViewerDocument {
    extensions: Vec<FileExtension>,
    route: PublicRoutePrefix,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PublicRouteDocument")]
struct PublicRouteDocument {
    prefix: PublicRoutePrefix,
    max_body_bytes: BodyLimit,
    requests_per_minute: RequestRate,
}

impl From<SurfacesDocument> for SurfacesDeclaration {
    fn from(document: SurfacesDocument) -> Self {
        Self {
            public_routes: document
                .public_routes
                .into_iter()
                .map(|route| PublicRouteDeclaration {
                    prefix: route.prefix,
                    max_body_bytes: route.max_body_bytes,
                    requests_per_minute: route.requests_per_minute,
                })
                .collect(),
            app_origin: document.app_origin,
            viewer: document.viewer.map(|viewer| ViewerDeclaration {
                extensions: viewer.extensions,
                route: viewer.route,
            }),
        }
    }
}
impl From<SurfacesDeclaration> for SurfacesDocument {
    fn from(declaration: SurfacesDeclaration) -> Self {
        Self {
            public_routes: declaration
                .public_routes
                .into_iter()
                .map(|route| PublicRouteDocument {
                    prefix: route.prefix,
                    max_body_bytes: route.max_body_bytes,
                    requests_per_minute: route.requests_per_minute,
                })
                .collect(),
            app_origin: declaration.app_origin,
            viewer: declaration.viewer.map(|viewer| ViewerDocument {
                extensions: viewer.extensions,
                route: viewer.route,
            }),
        }
    }
}
