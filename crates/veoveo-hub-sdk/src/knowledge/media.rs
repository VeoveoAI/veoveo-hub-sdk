//! The parts of a query or of an item to embed: text, or media a caller signed for a few
//! minutes. A signed URL and the headers its signature binds are credentials: their
//! `Debug` output is redacted, they implement no `Display`, and every error that refuses
//! one names the rule it broke, never the value.
use std::{borrow::Cow, collections::BTreeMap, fmt, num::NonZeroU64};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The most parts a query or an item to embed holds.
pub const MAX_PARTS: usize = 16;
/// The most bytes a signed URL holds.
pub const MAX_SIGNED_URL_BYTES: usize = 8192;
/// The most headers a signed request binds.
pub const MAX_MEDIA_HEADERS: usize = 32;
/// The most bytes of one header name.
pub const MAX_HEADER_NAME_BYTES: usize = 256;
/// The most bytes of one header value.
pub const MAX_HEADER_VALUE_BYTES: usize = 8192;
/// The largest media in bytes: 5 TiB, the largest object S3 stores.
pub const MAX_MEDIA_BYTES: u64 = 5 * 1024 * 1024 * 1024 * 1024;

/// Why a value is not a part of the knowledge contract. The message names the rule,
/// never the value, so a refused signed URL is never echoed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KnowledgeValueError(&'static str);

impl KnowledgeValueError {
    pub(crate) const fn new(rule: &'static str) -> Self {
        Self(rule)
    }

    /// The rule the value broke.
    pub fn rule(&self) -> &'static str {
        self.0
    }
}

/// What a part holds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
}

impl Modality {
    pub const ALL: [Self; 4] = [Self::Text, Self::Image, Self::Audio, Self::Video];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    /// Whether media of `media_type` is of this modality: an image is `image/*`, a video
    /// `video/*`, and audio `audio/*` or a video's soundtrack, `video/*`.
    pub fn admits(self, media_type: &MediaType) -> bool {
        match self {
            Self::Text => false,
            Self::Image => media_type.top_level() == "image",
            Self::Audio => matches!(media_type.top_level(), "audio" | "video"),
            Self::Video => media_type.top_level() == "video",
        }
    }
}

/// One ordered part: text, or an image, audio or video a caller signed.
///
/// On the wire a part is `{"type": "text", "text": …}`, or its media's members beside its
/// type, such as `{"type": "image", "url": …, "mediaType": "image/png", "size": 1024}`.
/// A media part's media type is of its type: an image is `image/*`, a video `video/*`,
/// and audio `audio/*` or a video's soundtrack, `video/*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ContentPart {
    Text { text: String },
    Image(SignedMedia),
    Audio(SignedMedia),
    Video(SignedMedia),
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// A media part of `modality`, whose media type must be of that modality.
    pub fn media(modality: Modality, media: SignedMedia) -> Result<Self, KnowledgeValueError> {
        let part = match modality {
            Modality::Text => {
                return Err(KnowledgeValueError::new(
                    "a media part is an image, audio or video",
                ));
            }
            Modality::Image => Self::Image(media),
            Modality::Audio => Self::Audio(media),
            Modality::Video => Self::Video(media),
        };
        part.check()?;
        Ok(part)
    }

    pub fn modality(&self) -> Modality {
        match self {
            Self::Text { .. } => Modality::Text,
            Self::Image(_) => Modality::Image,
            Self::Audio(_) => Modality::Audio,
            Self::Video(_) => Modality::Video,
        }
    }

    /// The signed media of a media part.
    pub fn signed_media(&self) -> Option<&SignedMedia> {
        match self {
            Self::Text { .. } => None,
            Self::Image(media) | Self::Audio(media) | Self::Video(media) => Some(media),
        }
    }

    fn check(&self) -> Result<(), KnowledgeValueError> {
        match self.signed_media() {
            Some(media) if !self.modality().admits(&media.media_type) => {
                Err(KnowledgeValueError::new(
                    "a media part's media type is of its type: an image is image/*, audio is \
                     audio/* or video/*, and a video is video/*",
                ))
            }
            _ => Ok(()),
        }
    }
}

/// 1 to [`MAX_PARTS`] ordered parts, each media part of its own modality: a query, or one
/// item to embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentParts(Vec<ContentPart>);

impl ContentParts {
    pub fn new(parts: Vec<ContentPart>) -> Result<Self, KnowledgeValueError> {
        if parts.is_empty() || parts.len() > MAX_PARTS {
            return Err(KnowledgeValueError::new("a query holds 1 to 16 parts"));
        }
        for part in &parts {
            part.check()?;
        }
        Ok(Self(parts))
    }

    /// One part alone.
    pub fn one(part: ContentPart) -> Result<Self, KnowledgeValueError> {
        Self::new(vec![part])
    }

    pub fn as_slice(&self) -> &[ContentPart] {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ContentPart> {
        self.0.iter()
    }

    pub fn into_vec(self) -> Vec<ContentPart> {
        self.0
    }

    /// Whether any part is of `modality`.
    pub fn holds(&self, modality: Modality) -> bool {
        self.0.iter().any(|part| part.modality() == modality)
    }
}

impl<'a> IntoIterator for &'a ContentParts {
    type Item = &'a ContentPart;
    type IntoIter = std::slice::Iter<'a, ContentPart>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Serialize for ContentParts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentParts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ContentParts {
    fn schema_name() -> Cow<'static, str> {
        "ContentParts".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let part = generator.subschema_for::<ContentPart>();
        json_schema!({
            "type": "array",
            "items": part,
            "minItems": 1,
            "maxItems": MAX_PARTS,
        })
    }
}

/// Media a caller signed for a few minutes: the URL to `GET`, the headers its signature
/// binds, which a fetch sends unchanged, and the media's type and size in bytes. The URL
/// and headers are credentials for as long as the signature lives.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedMedia {
    pub url: SignedUrl,
    #[serde(default, skip_serializing_if = "MediaHeaders::is_empty")]
    pub headers: MediaHeaders,
    pub media_type: MediaType,
    pub size: MediaSize,
}

impl SignedMedia {
    /// Media at `url` without signed headers.
    pub fn new(url: SignedUrl, media_type: MediaType, size: MediaSize) -> Self {
        Self {
            url,
            headers: MediaHeaders::default(),
            media_type,
            size,
        }
    }
}

impl fmt::Debug for SignedMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedMedia")
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("media_type", &self.media_type)
            .field("size", &self.size)
            .finish()
    }
}

/// An absolute `https` or `http` URL of at most [`MAX_SIGNED_URL_BYTES`] bytes, without
/// whitespace or control characters, kept exactly as signed. `Debug` shows
/// `SignedUrl([REDACTED])`; [`SignedUrl::expose`] is the one way to read it.
#[derive(Clone)]
pub struct SignedUrl(SecretString);

impl SignedUrl {
    pub fn parse(value: impl Into<String>) -> Result<Self, KnowledgeValueError> {
        let value = SecretString::from(value.into());
        let text = value.expose_secret();
        let admitted = text.len() <= MAX_SIGNED_URL_BYTES
            && (text.starts_with("https://") || text.starts_with("http://"))
            && !text.chars().any(|c| c.is_control() || c.is_whitespace())
            && url::Url::parse(text).is_ok_and(|url| url.has_host());
        if !admitted {
            return Err(KnowledgeValueError::new(
                "a signed URL is an absolute https or http URL of at most 8192 bytes, without \
                 whitespace or control characters",
            ));
        }
        Ok(Self(value))
    }

    /// The URL, for the one request that fetches the media.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl PartialEq for SignedUrl {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}
impl Eq for SignedUrl {}

impl fmt::Debug for SignedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignedUrl([REDACTED])")
    }
}

impl Serialize for SignedUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for SignedUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for SignedUrl {
    fn schema_name() -> Cow<'static, str> {
        "SignedUrl".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "uri",
            "maxLength": MAX_SIGNED_URL_BYTES,
            "description": "An absolute https or http URL that a signature binds; a credential while it lives.",
        })
    }
}

/// The headers a signed request binds: at most [`MAX_MEDIA_HEADERS`], each an RFC 9110
/// field name and a value without control characters but horizontal tab. `Debug` shows
/// only how many there are.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MediaHeaders(BTreeMap<HeaderName, HeaderValue>);

impl MediaHeaders {
    pub fn new(
        headers: impl IntoIterator<Item = (HeaderName, HeaderValue)>,
    ) -> Result<Self, KnowledgeValueError> {
        let headers: BTreeMap<_, _> = headers.into_iter().collect();
        if headers.len() > MAX_MEDIA_HEADERS {
            return Err(KnowledgeValueError::new(
                "a signed request binds at most 32 headers",
            ));
        }
        Ok(Self(headers))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Each header, for the one request that fetches the media.
    pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.0.iter()
    }
}

impl fmt::Debug for MediaHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MediaHeaders([REDACTED; {}])", self.0.len())
    }
}

impl Serialize for MediaHeaders {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MediaHeaders {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let headers = BTreeMap::<HeaderName, HeaderValue>::deserialize(deserializer)?;
        Self::new(headers).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for MediaHeaders {
    fn schema_name() -> Cow<'static, str> {
        "MediaHeaders".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "propertyNames": {
                "type": "string",
                "pattern": HEADER_NAME_PATTERN,
                "maxLength": MAX_HEADER_NAME_BYTES,
            },
            "additionalProperties": {"type": "string", "maxLength": MAX_HEADER_VALUE_BYTES},
            "maxProperties": MAX_MEDIA_HEADERS,
            "description": "The headers the signature binds, sent unchanged; credentials while the signature lives.",
        })
    }
}

/// An RFC 9110 `token`: the characters an HTTP field name may hold.
const HEADER_NAME_PATTERN: &str = r"^[!#$%&'*+.^_`|~0-9A-Za-z-]+$";

/// An HTTP field name, an RFC 9110 `token` of at most [`MAX_HEADER_NAME_BYTES`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HeaderName(String);

impl HeaderName {
    pub fn parse(value: impl Into<String>) -> Result<Self, KnowledgeValueError> {
        let value = value.into();
        let token = |byte: u8| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte);
        if value.is_empty() || value.len() > MAX_HEADER_NAME_BYTES || !value.bytes().all(token) {
            return Err(KnowledgeValueError::new(
                "a header name is an RFC 9110 token of at most 256 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HeaderName {
    type Error = KnowledgeValueError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<HeaderName> for String {
    fn from(value: HeaderName) -> Self {
        value.0
    }
}

/// An HTTP field value of at most [`MAX_HEADER_VALUE_BYTES`] bytes, without control
/// characters but horizontal tab. `Debug` shows `HeaderValue([REDACTED])`.
#[derive(Clone)]
pub struct HeaderValue(SecretString);

impl HeaderValue {
    pub fn parse(value: impl Into<String>) -> Result<Self, KnowledgeValueError> {
        let value = SecretString::from(value.into());
        let text = value.expose_secret();
        if text.len() > MAX_HEADER_VALUE_BYTES || text.chars().any(|c| c.is_control() && c != '\t')
        {
            return Err(KnowledgeValueError::new(
                "a header value holds at most 8192 bytes without control characters",
            ));
        }
        Ok(Self(value))
    }

    /// The value, for the one request that fetches the media.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl PartialEq for HeaderValue {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}
impl Eq for HeaderValue {}

impl fmt::Debug for HeaderValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HeaderValue([REDACTED])")
    }
}

impl Serialize for HeaderValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for HeaderValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A bare RFC 6838 media type, `type/subtype` in lowercase without parameters, such as
/// `image/png`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MediaType(String);

/// RFC 6838 `restricted-name` in lowercase, on both sides of the `/`.
const MEDIA_TYPE_PATTERN: &str =
    r"^[a-z0-9][a-z0-9!#$&^_.+-]{0,126}/[a-z0-9][a-z0-9!#$&^_.+-]{0,126}$";

impl MediaType {
    pub fn parse(value: impl Into<String>) -> Result<Self, KnowledgeValueError> {
        let value = value.into();
        let admitted = value
            .split_once('/')
            .is_some_and(|(kind, subtype)| restricted_name(kind) && restricted_name(subtype));
        if !admitted {
            return Err(KnowledgeValueError::new(
                "a media type is a lowercase RFC 6838 type/subtype without parameters",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The top-level type, such as `image`.
    pub fn top_level(&self) -> &str {
        self.0.split_once('/').map_or("", |(kind, _)| kind)
    }
}

/// RFC 6838 `restricted-name` in lowercase: a letter or digit, then up to 126 of the
/// listed characters.
fn restricted_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 127
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&-^_.+".contains(byte)
        })
}

impl TryFrom<String> for MediaType {
    type Error = KnowledgeValueError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<MediaType> for String {
    fn from(value: MediaType) -> Self {
        value.0
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for MediaType {
    fn schema_name() -> Cow<'static, str> {
        "MediaType".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": MEDIA_TYPE_PATTERN,
            "description": "A lowercase RFC 6838 type/subtype without parameters, such as image/png.",
        })
    }
}

/// A media's size in bytes, from 1 to [`MAX_MEDIA_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaSize(NonZeroU64);

impl MediaSize {
    pub fn new(bytes: u64) -> Result<Self, KnowledgeValueError> {
        NonZeroU64::new(bytes)
            .filter(|bytes| bytes.get() <= MAX_MEDIA_BYTES)
            .map(Self)
            .ok_or(KnowledgeValueError::new(
                "a media's size is 1 byte to 5 TiB",
            ))
    }

    pub fn bytes(self) -> u64 {
        self.0.get()
    }
}

impl Serialize for MediaSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.bytes())
    }
}

impl<'de> Deserialize<'de> for MediaSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for MediaSize {
    fn schema_name() -> Cow<'static, str> {
        "MediaSize".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "minimum": 1,
            "maximum": MAX_MEDIA_BYTES,
            "description": "The media's size in bytes.",
        })
    }
}
