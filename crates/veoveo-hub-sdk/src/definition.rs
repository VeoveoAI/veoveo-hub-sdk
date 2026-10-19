//! Runtime-free output cardinality and shared collection request construction.
use crate::calls::{ProviderContract, ResourceRead, ToolCall};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use veoveo_types::{Collection, CursorToken, Page, PageLimit, ResourceAddress, ResourceUri};

/// An explicitly declared structured single value, never a blanket Serialize impl.
/// ```compile_fail
/// fn single<T: veoveo_hub_sdk::definition::SingleOutput>() {}
/// single::<Vec<String>>();
/// ```
pub trait SingleOutput: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static {
    fn product_uri(&self) -> Option<&ResourceUri> {
        None
    }
}
impl<T> SingleOutput for veoveo_types::Checked<T>
where
    T: veoveo_types::Check + SingleOutput,
    T::Error: std::fmt::Display,
{
    fn product_uri(&self) -> Option<&ResourceUri> {
        SingleOutput::product_uri(self.get())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoFilters {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    Single,
    Collection,
}
pub trait Output: Serialize + DeserializeOwned + JsonSchema + Send + 'static {
    const CARDINALITY: Cardinality;
    fn product_uri(&self) -> Option<&ResourceUri> {
        None
    }
}
impl<T: SingleOutput> Output for T {
    const CARDINALITY: Cardinality = Cardinality::Single;
    fn product_uri(&self) -> Option<&ResourceUri> {
        SingleOutput::product_uri(self)
    }
}
impl<C: Collection> Output for Page<C>
where
    C::Item: Serialize + DeserializeOwned + JsonSchema + Send,
{
    const CARDINALITY: Cardinality = Cardinality::Collection;
}

pub trait CollectionTool: Send + Sync + 'static {
    type Provider: ProviderContract;
    type Collection: Collection;
    const TOOL: &'static str;
    fn default_limit() -> <Self::Collection as Collection>::Limit {
        maximum::<Self::Collection>()
    }
}
fn maximum<C: Collection>() -> C::Limit {
    C::Limit::new(C::Limit::MAX).expect("native maximum is admitted")
}

/// Admitted shared URI selection and the remaining owner path/filter URI.
pub type ParsedCollectionSelection<C> = (
    ResourceUri,
    <C as Collection>::Limit,
    Option<CursorToken<C>>,
);

/// One common selection for tools and resources. Filters are owner-typed; limit,
/// cursor admission, encoding and duplicate-query refusal are shared mechanics.
pub struct CollectionRequest<C: Collection> {
    filters: C::Filters,
    limit: C::Limit,
    cursor: Option<CursorToken<C>>,
}
impl<C: Collection> CollectionRequest<C> {
    pub fn new(filters: C::Filters, limit: C::Limit) -> Self {
        Self {
            filters,
            limit,
            cursor: None,
        }
    }
    pub fn continuing(mut self, cursor: CursorToken<C>) -> Self {
        self.cursor = Some(cursor);
        self
    }
    pub fn filters(&self) -> &C::Filters {
        &self.filters
    }
    pub fn limit(&self) -> C::Limit {
        self.limit
    }
    pub fn cursor(&self) -> Option<&CursorToken<C>> {
        self.cursor.as_ref()
    }
    pub fn into_parts(self) -> (C::Filters, C::Limit, Option<CursorToken<C>>) {
        (self.filters, self.limit, self.cursor)
    }
    pub fn uri(&self, base: &ResourceUri) -> Result<ResourceUri, CollectionRequestError> {
        base.components().map_err(|_| CollectionRequestError)?;
        let mut uri = url::Url::parse(base.as_str()).map_err(|_| CollectionRequestError)?;
        if uri
            .query_pairs()
            .any(|(key, _)| matches!(key.as_ref(), "limit" | "cursor"))
        {
            return Err(CollectionRequestError);
        }
        uri.query_pairs_mut()
            .append_pair("limit", &self.limit.get().to_string());
        if let Some(cursor) = &self.cursor {
            uri.query_pairs_mut().append_pair("cursor", cursor.as_str());
        }
        ResourceUri::new(uri.as_str()).map_err(|_| CollectionRequestError)
    }
    pub fn base_uri(uri: &ResourceUri) -> Result<ResourceUri, CollectionRequestError> {
        uri.components().map_err(|_| CollectionRequestError)?;
        let mut parsed = url::Url::parse(uri.as_str()).map_err(|_| CollectionRequestError)?;
        let other: Vec<_> = parsed
            .query_pairs()
            .filter(|(key, _)| !matches!(key.as_ref(), "limit" | "cursor"))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        parsed.set_query(None);
        if !other.is_empty() {
            parsed.query_pairs_mut().extend_pairs(other);
        }
        ResourceUri::new(parsed.as_str()).map_err(|_| CollectionRequestError)
    }
    /// Strip and admit shared query arguments before the owner parses only its
    /// path and filters. Other query values are preserved with URI encoding.
    pub fn split_uri(
        uri: &ResourceUri,
        default: C::Limit,
    ) -> Result<ParsedCollectionSelection<C>, CollectionRequestError> {
        uri.components().map_err(|_| CollectionRequestError)?;
        let mut parsed = url::Url::parse(uri.as_str()).map_err(|_| CollectionRequestError)?;
        let mut limit = None;
        let mut cursor = None;
        let mut other = vec![];
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "limit" => {
                    if limit.is_some() {
                        return Err(CollectionRequestError);
                    }
                    limit = Some(
                        C::Limit::new(value.parse().map_err(|_| CollectionRequestError)?)
                            .map_err(|_| CollectionRequestError)?,
                    );
                }
                "cursor" => {
                    if cursor.is_some() {
                        return Err(CollectionRequestError);
                    }
                    cursor = Some(
                        serde_json::from_value::<CursorToken<C>>(serde_json::Value::String(
                            value.into_owned(),
                        ))
                        .map_err(|_| CollectionRequestError)?,
                    );
                }
                _ => other.push((key.into_owned(), value.into_owned())),
            }
        }
        parsed.set_query(None);
        if !other.is_empty() {
            parsed.query_pairs_mut().extend_pairs(other);
        }
        Ok((
            ResourceUri::new(parsed.as_str()).map_err(|_| CollectionRequestError)?,
            limit.unwrap_or(default),
            cursor,
        ))
    }
}
#[derive(Debug, thiserror::Error)]
#[error("invalid collection limit or cursor query")]
pub struct CollectionRequestError;

pub struct List<T: CollectionTool> {
    selection: CollectionRequest<T::Collection>,
    tool: PhantomData<fn() -> T>,
}
impl<T: CollectionTool> List<T> {
    pub fn new(
        filters: <T::Collection as Collection>::Filters,
        limit: <T::Collection as Collection>::Limit,
    ) -> Self {
        Self {
            selection: CollectionRequest::new(filters, limit),
            tool: PhantomData,
        }
    }
    pub fn continuing(mut self, cursor: CursorToken<T::Collection>) -> Self {
        self.selection = self.selection.continuing(cursor);
        self
    }
    pub fn selection(&self) -> &CollectionRequest<T::Collection> {
        &self.selection
    }
    pub fn into_selection(self) -> CollectionRequest<T::Collection> {
        self.selection
    }
}
impl<T: CollectionTool> Serialize for List<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a, F, L, N> {
            filters: &'a F,
            limit: L,
            #[serde(skip_serializing_if = "Option::is_none")]
            cursor: Option<&'a N>,
        }
        Wire {
            filters: self.selection.filters(),
            limit: self.selection.limit(),
            cursor: self.selection.cursor(),
        }
        .serialize(serializer)
    }
}
impl<'de, T: CollectionTool> Deserialize<'de> for List<T>
where
    <T::Collection as Collection>::Filters: Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire<F, L, N> {
            filters: F,
            limit: Option<L>,
            cursor: Option<N>,
        }
        let wire = Wire::<
            <T::Collection as Collection>::Filters,
            <T::Collection as Collection>::Limit,
            CursorToken<T::Collection>,
        >::deserialize(deserializer)?;
        Ok(Self {
            selection: CollectionRequest {
                filters: wire.filters,
                limit: wire.limit.unwrap_or_else(T::default_limit),
                cursor: wire.cursor,
            },
            tool: PhantomData,
        })
    }
}
impl<T: CollectionTool> JsonSchema for List<T>
where
    <T::Collection as Collection>::Filters: JsonSchema,
{
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("{}Request", T::TOOL).into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        std::any::type_name::<Self>().into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        #[derive(JsonSchema)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Shape<F, L> {
            filters: F,
            limit: Option<L>,
            cursor: Option<String>,
        }
        let mut schema = Shape::<
            <T::Collection as Collection>::Filters,
            <T::Collection as Collection>::Limit,
        >::json_schema(generator);
        if let Some(properties) = schema
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
        {
            properties.insert(
                "cursor".into(),
                serde_json::json!({"type":"string","minLength":1,"maxLength":4096}),
            );
            if let Some(limit) = properties
                .get_mut("limit")
                .and_then(serde_json::Value::as_object_mut)
            {
                limit.insert("default".into(), T::default_limit().get().into());
            }
        }
        schema
    }
}
impl<T: CollectionTool> ToolCall for List<T>
where
    <T::Collection as Collection>::Item: Serialize + DeserializeOwned + JsonSchema + Send,
    <T::Collection as Collection>::Filters: Send + Sync,
{
    type Provider = T::Provider;
    type Output = Page<T::Collection>;
    const TOOL: &'static str = T::TOOL;
}

/// Owner maps path/filter parameters only. Shared selection query handling is
/// supplied by ReadCollection and the typed resource transport adapter.
pub trait CollectionResource: Send + Sync + 'static {
    type Provider: ProviderContract;
    type Address: ResourceAddress;
    type Collection: Collection;
    const TEMPLATE: &'static str;
    fn address(&self) -> Self::Address;
    fn filters(&self) -> <Self::Collection as Collection>::Filters;
    fn from_address(address: &Self::Address) -> Option<Self>
    where
        Self: Sized;
    fn default_limit() -> <Self::Collection as Collection>::Limit {
        maximum::<Self::Collection>()
    }
}
/// Add the shared paging arguments to an owner path/filter template. The native
/// collection profile permits one trailing optional query expression; paging
/// variables are supplied here once for discovery and route authorization.
pub fn collection_template(
    owner: &str,
) -> Result<veoveo_types::ResourceTemplateUri, CollectionRequestError> {
    let template =
        veoveo_types::ResourceTemplateUri::new(owner).map_err(|_| CollectionRequestError)?;
    if template
        .variables()
        .any(|name| matches!(name, "limit" | "cursor"))
        || owner.contains("{&")
    {
        return Err(CollectionRequestError);
    }
    let result = if let Some((path, query)) = owner.split_once("{?") {
        let names = query.strip_suffix('}').ok_or(CollectionRequestError)?;
        if path.contains('?') || names.is_empty() || names.contains(['{', '}', '*', ':']) {
            return Err(CollectionRequestError);
        }
        format!("{path}{{?{names},limit,cursor}}")
    } else {
        if owner.contains('?') {
            return Err(CollectionRequestError);
        }
        format!("{owner}{{?limit,cursor}}")
    };
    veoveo_types::ResourceTemplateUri::new(result).map_err(|_| CollectionRequestError)
}

pub struct ReadCollection<R: CollectionResource> {
    resource: R,
    selection: CollectionRequest<R::Collection>,
}
impl<R: CollectionResource> ReadCollection<R> {
    pub fn new(resource: R, limit: <R::Collection as Collection>::Limit) -> Self {
        let selection = CollectionRequest::new(resource.filters(), limit);
        Self {
            resource,
            selection,
        }
    }
    pub fn continuing(mut self, cursor: CursorToken<R::Collection>) -> Self {
        self.selection = self.selection.continuing(cursor);
        self
    }
    pub fn resource(&self) -> &R {
        &self.resource
    }
    pub fn selection(&self) -> &CollectionRequest<R::Collection> {
        &self.selection
    }
    pub fn into_selection(self) -> CollectionRequest<R::Collection> {
        self.selection
    }
}
impl<R: CollectionResource> ResourceRead for ReadCollection<R>
where
    <R::Collection as Collection>::Item: Serialize + DeserializeOwned + JsonSchema + Send,
    <R::Collection as Collection>::Filters: Send + Sync,
{
    type Provider = R::Provider;
    type Address = R::Address;
    type Content = Page<R::Collection>;
    fn address(&self) -> R::Address {
        self.resource.address()
    }
    fn uri(&self) -> Result<ResourceUri, crate::calls::CallContractError> {
        self.selection
            .uri(
                &self
                    .resource
                    .address()
                    .to_uri()
                    .map_err(|e| crate::calls::CallContractError::Address(e.to_string()))?,
            )
            .map_err(|e| crate::calls::CallContractError::Address(e.to_string()))
    }
}
pub trait ResourceContent: Output {}
impl<T: Output> ResourceContent for T {}

pub trait TemplateRead: ResourceRead + Sized {
    const TEMPLATE: &'static str;
    fn from_address(address: &Self::Address) -> Option<Self>;
}

/// One canonical addressable product. The URI field and matching MCP link are
/// generated together by the output adapter; metadata cannot replace either.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent, bound = "T:SingleOutput")]
pub struct Product<T: SingleOutput>(veoveo_types::Checked<ProductValue<T>>);
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound = "T:SingleOutput"
)]
struct ProductValue<T: SingleOutput> {
    result_uri: ResourceUri,
    metadata: T,
}
impl<T: SingleOutput> veoveo_types::Check for ProductValue<T> {
    type Error = veoveo_types::ResourceUriError;
    fn check(&self) -> Result<(), Self::Error> {
        self.result_uri.components().map(|_| ())
    }
}
impl<T: SingleOutput> Product<T> {
    pub fn new(
        address: &impl ResourceAddress,
        metadata: T,
    ) -> Result<Self, crate::calls::CallContractError> {
        let result_uri = address
            .to_uri()
            .map_err(|error| crate::calls::CallContractError::Address(error.to_string()))?;
        veoveo_types::Checked::new(ProductValue {
            result_uri,
            metadata,
        })
        .map(Self)
        .map_err(|error| crate::calls::CallContractError::Address(error.to_string()))
    }
    pub fn metadata(&self) -> &T {
        &self.0.metadata
    }
    pub fn result_uri(&self) -> &ResourceUri {
        &self.0.result_uri
    }
}
impl<T: SingleOutput> SingleOutput for Product<T> {
    fn product_uri(&self) -> Option<&ResourceUri> {
        Some(self.result_uri())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_query_template_merges_optional_filters_without_a_leading_ampersand() {
        assert_eq!(
            collection_template("fixture://items").unwrap().as_str(),
            "fixture://items{?limit,cursor}"
        );
        assert_eq!(
            collection_template("fixture://owners/{owner}/items{?state,name}")
                .unwrap()
                .as_str(),
            "fixture://owners/{owner}/items{?state,name,limit,cursor}"
        );
        for bad in [
            "fixture://items{?cursor}",
            "fixture://items{?limit}",
            "fixture://items{&state}",
            "fixture://items{?state}{?name}",
            "fixture://items?state=fixed",
            "fixture://items{?state*}",
        ] {
            assert!(collection_template(bad).is_err(), "{bad}");
        }
    }
}
