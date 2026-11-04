//! Embeddings: one vector per item of ordered parts, and the model that made them, so
//! that vectors of different models are never compared. The SDK names no server, so it
//! only declares [`Embedder`]; the Embedding server's `client` feature implements it.
use std::{borrow::Cow, num::NonZeroU32};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use veoveo_types::{Check, Checked, IdentifierError};

/// The most bytes of a model checkpoint's name.
pub const MAX_CHECKPOINT_BYTES: usize = 256;

/// What the vectors are for: a record's own vector, or a query's.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingPurpose {
    /// A record's text and media together, stored beside the record.
    Document,
    /// A query, compared with the documents' vectors.
    Query,
}

/// The checkpoint a model's weights come from, such as
/// `google/embeddinggemma-300m@64614b0b`: 1 to [`MAX_CHECKPOINT_BYTES`] bytes without
/// whitespace or control characters.
#[veoveo_types::id(text(Checkpoints))]
pub struct ModelCheckpoint(String);

#[doc(hidden)]
pub struct Checkpoints;
impl veoveo_types::IdProfile for Checkpoints {
    type Error = IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(|value, _| validate_checkpoint(value));
}

fn validate_checkpoint(value: &str) -> Result<(), IdentifierError> {
    veoveo_types::identifier_syntax::validate_token_text(value)?;
    if value.len() > MAX_CHECKPOINT_BYTES {
        return Err(IdentifierError::new(value, "must be at most 256 bytes"));
    }
    Ok(())
}

/// How many components each vector has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmbeddingDimensions(NonZeroU32);

impl EmbeddingDimensions {
    pub fn new(dimensions: u32) -> Option<Self> {
        NonZeroU32::new(dimensions).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl Serialize for EmbeddingDimensions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.get())
    }
}

impl<'de> Deserialize<'de> for EmbeddingDimensions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u32::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("an embedding has at least one dimension"))
    }
}

impl JsonSchema for EmbeddingDimensions {
    fn schema_name() -> Cow<'static, str> {
        "EmbeddingDimensions".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "minimum": 1, "maximum": u32::MAX})
    }
}

/// The model that made a set of vectors: its checkpoint and the vectors' dimensions. Two
/// vectors are comparable only when they share it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingModel {
    pub checkpoint: ModelCheckpoint,
    pub dimensions: EmbeddingDimensions,
}

/// One vector per item, in the items' order, and the model that made them. Every vector
/// has the model's dimensions and finite components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Embeddings(Checked<EmbeddingsValue>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "Embeddings", inline)]
struct EmbeddingsValue {
    model: EmbeddingModel,
    vectors: Vec<Vec<f32>>,
}

/// Why vectors do not fit their model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("each vector has the model's dimensions and finite components")]
pub struct EmbeddingsError;

impl Check for EmbeddingsValue {
    type Error = EmbeddingsError;
    fn check(&self) -> Result<(), Self::Error> {
        let dimensions = self.model.dimensions.get() as usize;
        if self
            .vectors
            .iter()
            .all(|vector| vector.len() == dimensions && vector.iter().all(|x| x.is_finite()))
        {
            Ok(())
        } else {
            Err(EmbeddingsError)
        }
    }
}

impl Embeddings {
    pub fn new(model: EmbeddingModel, vectors: Vec<Vec<f32>>) -> Result<Self, EmbeddingsError> {
        Checked::new(EmbeddingsValue { model, vectors }).map(Self)
    }

    pub fn model(&self) -> &EmbeddingModel {
        &self.0.model
    }

    /// The vectors, one per item in the items' order.
    pub fn vectors(&self) -> &[Vec<f32>] {
        &self.0.vectors
    }

    pub fn into_vectors(self) -> Vec<Vec<f32>> {
        self.0.into_inner().vectors
    }
}

#[cfg(feature = "client")]
pub use embedder::Embedder;

#[cfg(feature = "client")]
mod embedder {
    use std::future::Future;

    use super::{EmbeddingPurpose, Embeddings};
    use crate::{client::HubClient, knowledge::ContentParts};

    /// Turns items of ordered parts into [`Embeddings`], one vector per item in the items'
    /// order, through `client`: the caller-bound client a handler's `ctx.client()` or a
    /// Task's worker already holds, so the caller is the one who embeds and the usage is
    /// theirs. The SDK implements none; the Embedding server's `client` feature does,
    /// and tests pass their own.
    pub trait Embedder: Send + Sync {
        type Error: std::error::Error + Send + Sync + 'static;

        fn embed(
            &self,
            client: &HubClient,
            items: &[ContentParts],
            purpose: EmbeddingPurpose,
        ) -> impl Future<Output = Result<Embeddings, Self::Error>> + Send;
    }
}
