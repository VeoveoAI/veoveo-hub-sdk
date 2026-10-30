//! Inverse matching for the native MCP route profile, distinct from policy selectors.
//! Domain address parsers still admit the captured field values.
use std::collections::BTreeSet;
use veoveo_types::{ResourceTemplateUri, ResourceUri, UriSegment};

use super::authorization_decision::{self, Path};

pub(crate) struct AuthorizationTemplate<'a> {
    scheme: &'a str,
    authority: &'a str,
    path: Vec<Path<&'a str>>,
    query: BTreeSet<&'a str>,
    trailing_slash: bool,
}

impl<'a> AuthorizationTemplate<'a> {
    /// Admit exactly the native route profile: literal root, whole scalar path
    /// segments, a terminal reserved tail and one terminal optional query expansion.
    pub(crate) fn parse(template: &'a ResourceTemplateUri) -> Option<Self> {
        let wire = template.as_str();
        let mut variables = BTreeSet::new();
        let (path_wire, query) = if let Some((path, query)) = wire.split_once("{?") {
            let names = query.strip_suffix('}')?;
            let mut query = BTreeSet::new();
            for name in names.split(',') {
                if !variable(name) || !variables.insert(name) || !query.insert(name) {
                    return None;
                }
            }
            (path, query)
        } else {
            (wire, BTreeSet::new())
        };
        let (scheme, rest) = path_wire.split_once("://")?;
        let (authority, path_wire) = rest
            .split_once('/')
            .map_or((rest, None), |(root, path)| (root, Some(path)));
        let root = ResourceUri::new(format!("{scheme}://{authority}")).ok()?;
        let root_parts = root.components().ok()?;
        if root_parts.scheme() != scheme
            || root_parts.authority() != authority
            || root_parts.has_query()
            || root_parts.path_segments().next().is_some()
        {
            return None;
        }
        if path_wire.is_some_and(|path| path.starts_with('/')) {
            return None;
        }
        let trailing_slash = path_wire.is_some_and(|path| path.is_empty() || path.ends_with('/'));
        let path_wire = path_wire.map(|path| {
            if trailing_slash {
                path.strip_suffix('/').unwrap_or(path)
            } else {
                path
            }
        });
        let mut path = Vec::new();
        if let Some(path_wire) = path_wire.filter(|path| !path.is_empty()) {
            let segments: Vec<_> = path_wire.split('/').collect();
            for (index, segment) in segments.iter().enumerate() {
                if let Some(name) = segment
                    .strip_prefix("{+")
                    .and_then(|part| part.strip_suffix('}'))
                {
                    if !variable(name) || !variables.insert(name) || index + 1 != segments.len() {
                        return None;
                    }
                    path.push(Path::Tail);
                } else if let Some(name) = segment
                    .strip_prefix('{')
                    .and_then(|part| part.strip_suffix('}'))
                {
                    if !variable(name) || !variables.insert(name) {
                        return None;
                    }
                    path.push(Path::Scalar);
                } else {
                    if segment.is_empty()
                        || segment.contains(['{', '}', '?', '#', '%'])
                        || UriSegment::new(*segment).is_err()
                    {
                        return None;
                    }
                    path.push(Path::Literal(*segment));
                }
            }
        }
        Some(Self {
            scheme,
            authority,
            path,
            query,
            trailing_slash,
        })
    }

    #[cfg(feature = "server")]
    pub(crate) fn scheme(&self) -> &str {
        self.scheme
    }
    #[cfg(feature = "server")]
    pub(crate) fn authority(&self) -> &str {
        self.authority
    }

    pub(super) fn matches(&self, uri: &ResourceUri) -> bool {
        let Ok(parts) = uri.components() else {
            return false;
        };
        if !authorization_decision::same_root(
            &(parts.scheme(), parts.authority()),
            &(self.scheme, self.authority),
        ) {
            return false;
        }
        let segments: Vec<_> = parts.path_segments().collect();
        authorization_decision::matches_view(
            authorization_decision::TemplateView {
                path: &self.path,
                query: &self.query,
                trailing_slash: self.trailing_slash,
            },
            authorization_decision::ResourceView {
                segments: &segments,
                has_query: parts.has_query(),
                query_empty: parts.query_parameters().is_empty(),
                query: parts.query_parameters().keys(),
            },
        )
    }
}

fn variable(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
}

impl authorization_decision::Segment<&str> for std::borrow::Cow<'_, str> {
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    fn literal_eq(&self, literal: &&str) -> bool {
        self.as_ref() == *literal
    }
}

impl<'a> authorization_decision::Cursor<&'a str>
    for std::collections::btree_map::Keys<'a, String, String>
{
    fn next(&mut self) -> Option<&'a str> {
        Iterator::next(self).map(String::as_str)
    }
}

impl<'a> authorization_decision::AllowedQuery<&'a str> for BTreeSet<&str> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn contains(&self, name: &&'a str) -> bool {
        self.contains(*name)
    }
}
