//! The knowledge contract in a Rust server: [`KnowledgeSearch`], which the routes register
//! with `RoutesBuilder::knowledge`, and the checks every result passes before it leaves
//! the server. Inside `search` a server may do anything; the SDK checks what it returns.
use std::{fmt, future::Future, sync::Arc};

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolResult, ContentBlock, JsonObject, MetaObject, Resource, ResourceTemplate, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
};
use veoveo_types::{ResourceTemplateUri, ResourceUri};

use super::{
    CollectionMarker, CollectionSelection, EXTENSION_ID, KnowledgeCollection, KnowledgeHit,
    KnowledgeLimit, KnowledgeQuery, KnowledgeResults, MAX_HITS, TOOL,
};
use crate::{
    hosting::{DomainServer, ToolError},
    registration::authorization_template::AuthorizationTemplate,
};

/// The description of `search_knowledge`, the same on every server.
pub const DESCRIPTION: &str = "Search this server's records by meaning and words. The query \
    is 1 to 16 ordered parts, each text or an image, audio or video the caller signed; it \
    can name the collections to search and the most hits, 1 to 20, 10 by default. Each hit \
    names a record, which the caller reads through this server.";

/// Why arguments the published schema admits are not a knowledge query. It names the
/// rules, never an argument, so a refused signed URL is never echoed.
pub(crate) const INVALID_QUERY: &str = "the arguments are not a knowledge query: 1 to 16 \
    parts, each text or an image, audio or video whose media type is of its type and whose \
    URL is an absolute https or http URL; at most 16 collections, each named once as \
    {slug}.{plural}; a limit of 1 to 20";

/// What a search runs with: the server's domain, which holds its state, and the request
/// it answers, which carries the verified caller and calls other servers for them.
pub struct SearchContext<D> {
    pub domain: D,
    pub request: RequestContext<RoleServer>,
}

/// A server's search of its own records. The routes check the caller's policy before it
/// runs and every hit after it returns: at most the query's limit, each hit's address one
/// a template marked with its collection serves, and its collection one the query named
/// when it named any. A hit that fails a check fails the call; it is never dropped.
pub trait KnowledgeSearch<D: DomainServer>: Send + Sync + 'static {
    /// The records that answer `query`, in rank order, that the caller may read.
    fn search(
        &self,
        context: SearchContext<D>,
        query: KnowledgeQuery,
    ) -> impl Future<Output = Result<Vec<KnowledgeHit>, ToolError>> + Send;
}

/// The input schema `search_knowledge` publishes: [`KnowledgeQuery`]'s, through
/// [`publish_input_schema`](crate::hosting::publish_input_schema).
pub fn input_schema() -> Arc<JsonObject> {
    let mut schema = rmcp::handler::server::tool::schema_for_type::<KnowledgeQuery>();
    crate::hosting::publish_input_schema(Arc::make_mut(&mut schema));
    schema
}

/// The output schema `search_knowledge` publishes: [`KnowledgeResults`]'.
pub fn output_schema() -> Arc<JsonObject> {
    rmcp::handler::server::tool::schema_for_type::<KnowledgeResults>()
}

/// The annotations of `search_knowledge`: a read, which changes nothing.
pub fn tool_annotations() -> ToolAnnotations {
    ToolAnnotations::new().read_only(true).destructive(false)
}

/// The descriptor of `search_knowledge`.
pub(crate) fn descriptor() -> Tool {
    let mut tool = Tool::new(TOOL, DESCRIPTION, input_schema());
    tool.output_schema = Some(output_schema());
    tool.annotations = Some(tool_annotations());
    tool
}

/// `descriptor` with `_meta["ai.veoveo/knowledge"]` naming `collection`.
pub(crate) fn mark(
    mut descriptor: ResourceTemplate,
    collection: &KnowledgeCollection,
) -> ResourceTemplate {
    let marker = CollectionMarker {
        collection: collection.clone(),
    };
    descriptor
        .meta
        .get_or_insert_with(MetaObject::new)
        .insert(EXTENSION_ID.to_owned(), marker.to_value());
    descriptor
}

/// A template the routes marked as a collection, and how it recognizes its addresses.
#[derive(Clone)]
pub(crate) struct MarkedTemplate {
    collection: KnowledgeCollection,
    template: ResourceTemplateUri,
    reads: Arc<dyn Fn(&ResourceUri) -> bool + Send + Sync>,
}

impl MarkedTemplate {
    /// `template`, marked as `collection`, whose route reads a URI when `reads` holds.
    pub(crate) fn new(
        collection: KnowledgeCollection,
        template: ResourceTemplateUri,
        reads: impl Fn(&ResourceUri) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            collection,
            template,
            reads: Arc::new(reads),
        }
    }

    pub(crate) fn collection(&self) -> &KnowledgeCollection {
        &self.collection
    }

    /// Whether `uri` is one of this template's addresses: it matches the template, as the
    /// Hub matches a read to its requirement, and the template's route parses it.
    pub(crate) fn addresses(&self, uri: &ResourceUri) -> bool {
        AuthorizationTemplate::parse(&self.template).is_some_and(|template| template.matches(uri))
            && (self.reads)(uri)
    }
}

/// Why a search's result cannot leave the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    TooManyHits,
    UnmarkedCollection,
    UnaddressedHit,
    UnnamedCollection,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooManyHits => "the knowledge search returned more hits than the query's limit",
            Self::UnmarkedCollection => {
                "a knowledge hit names a collection that no template of this server marks"
            }
            Self::UnaddressedHit => {
                "a knowledge hit's address is not one its collection's template serves"
            }
            Self::UnnamedCollection => "a knowledge hit's collection is not one the query names",
        })
    }
}

/// The hits as the result that leaves the server, or the check they fail.
fn checked(
    marked: &[&MarkedTemplate],
    query: &Admitted,
    hits: Vec<KnowledgeHit>,
) -> Result<KnowledgeResults, Refusal> {
    if hits.len() > query.limit.get() as usize {
        return Err(Refusal::TooManyHits);
    }
    for hit in &hits {
        let Some(template) = marked
            .iter()
            .find(|template| *template.collection() == hit.collection)
        else {
            return Err(Refusal::UnmarkedCollection);
        };
        if !template.addresses(&hit.uri) {
            return Err(Refusal::UnaddressedHit);
        }
        if !query.collections.is_empty() && !query.collections.contains(&hit.collection) {
            return Err(Refusal::UnnamedCollection);
        }
    }
    KnowledgeResults::new(hits).map_err(|_| Refusal::TooManyHits)
}

/// What the checks keep of a query the search consumes.
struct Admitted {
    collections: CollectionSelection,
    limit: KnowledgeLimit,
}

/// Answers an admitted `query` for `D`'s routes with `search`: refuses a collection the
/// routes do not mark as invalid params, runs the search and returns its checked hits,
/// with one resource link per hit.
pub(crate) async fn answer<D: DomainServer, S: KnowledgeSearch<D>>(
    search: &S,
    domain: D,
    query: KnowledgeQuery,
    request: RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    let marked: Vec<&MarkedTemplate> = D::routes().marked_templates().collect();
    if let Some(unmarked) = query.collections.iter().find(|named| {
        marked
            .iter()
            .all(|template| template.collection() != *named)
    }) {
        return Err(ErrorData::invalid_params(
            format!("the query names `{unmarked}`, which no template of this server marks"),
            None,
        ));
    }
    let admitted = Admitted {
        collections: query.collections.clone(),
        limit: query.limit,
    };
    let hits = match search
        .search(SearchContext { domain, request }, query)
        .await
    {
        Ok(hits) => hits,
        Err(ToolError::Protocol(error)) => return Err(error),
        Err(ToolError::Rejected(message)) => {
            return Ok(CallToolResult::error(vec![ContentBlock::text(message)]));
        }
    };
    let results = checked(&marked, &admitted, hits).map_err(|refusal| {
        tracing::warn!(%refusal, "a knowledge search result failed its checks");
        ErrorData::internal_error(refusal.to_string(), None)
    })?;
    respond(&results)
}

/// The result of `search_knowledge`: the hits as structured content, and one resource
/// link per hit, in rank order.
fn respond(results: &KnowledgeResults) -> Result<CallToolResult, ErrorData> {
    let value = serde_json::to_value(results)
        .map_err(|_| ErrorData::internal_error("knowledge results serialization failed", None))?;
    let mut result = CallToolResult::structured(value);
    result.content.extend(results.hits().iter().map(|hit| {
        ContentBlock::resource_link(
            Resource::new(hit.uri.as_str(), hit.title.clone()).with_mime_type("application/json"),
        )
    }));
    Ok(result)
}

// A query that names no limit asks for no more than the most hits.
const _: () = assert!(super::DEFAULT_LIMIT <= MAX_HITS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{ContentPart, ContentParts, Snippet};

    fn collection(name: &str) -> KnowledgeCollection {
        KnowledgeCollection::parse(name).unwrap()
    }

    fn marked(name: &str, template: &str) -> MarkedTemplate {
        MarkedTemplate::new(
            collection(name),
            ResourceTemplateUri::new(template).unwrap(),
            |_| true,
        )
    }

    fn hit(uri: &str, name: &str, snippet: &str) -> KnowledgeHit {
        KnowledgeHit {
            uri: ResourceUri::new(uri).unwrap(),
            collection: collection(name),
            title: "A record".into(),
            snippet: Snippet::new(snippet),
            modified_at: None,
        }
    }

    fn admitted(collections: &[&str], limit: u32) -> Admitted {
        Admitted {
            collections: CollectionSelection::new(
                collections.iter().map(|name| collection(name)).collect(),
            )
            .unwrap(),
            limit: KnowledgeLimit::new(limit).unwrap(),
        }
    }

    #[test]
    fn hits_within_the_limit_on_their_marked_templates_leave_the_server() {
        let items = marked("shop.items", "shop://items/{item_id}");
        let parts = marked("shop.parts", "shop://parts/{part_id}");
        let templates = [&items, &parts];
        let results = checked(
            &templates,
            &admitted(&[], 2),
            vec![
                hit("shop://items/one", "shop.items", "first"),
                hit("shop://parts/two", "shop.parts", "second"),
            ],
        )
        .unwrap();
        assert_eq!(results.hits().len(), 2);
        let link = respond(&results).unwrap();
        let links: Vec<&str> = link
            .content
            .iter()
            .filter_map(|content| content.as_resource_link())
            .map(|link| link.uri.as_str())
            .collect();
        assert_eq!(links, ["shop://items/one", "shop://parts/two"]);
        assert_eq!(
            link.structured_content.unwrap()["hits"][1]["collection"],
            "shop.parts"
        );
    }

    #[test]
    fn a_hit_that_fails_a_check_fails_the_result() {
        let items = marked("shop.items", "shop://items/{item_id}");
        let parts = marked("shop.parts", "shop://parts/{part_id}");
        let templates = [&items, &parts];
        let one = || hit("shop://items/one", "shop.items", "first");
        assert_eq!(
            checked(&templates, &admitted(&[], 1), vec![one(), one()]),
            Err(Refusal::TooManyHits)
        );
        assert_eq!(
            checked(
                &templates,
                &admitted(&[], 5),
                vec![one(), hit("shop://items/two", "shop.orders", "x")]
            ),
            Err(Refusal::UnmarkedCollection)
        );
        // An address of another template, of no template, or that the route refuses.
        for uri in ["shop://parts/one", "shop://items/one/more", "shop://items"] {
            assert_eq!(
                checked(
                    &templates,
                    &admitted(&[], 5),
                    vec![hit(uri, "shop.items", "x")]
                ),
                Err(Refusal::UnaddressedHit),
                "{uri}"
            );
        }
        let refusing = MarkedTemplate::new(
            collection("shop.items"),
            ResourceTemplateUri::new("shop://items/{item_id}").unwrap(),
            |_| false,
        );
        assert_eq!(
            checked(&[&refusing], &admitted(&[], 5), vec![one()]),
            Err(Refusal::UnaddressedHit)
        );
        assert_eq!(
            checked(
                &templates,
                &admitted(&["shop.parts"], 5),
                vec![hit("shop://parts/two", "shop.parts", "x"), one()]
            ),
            Err(Refusal::UnnamedCollection)
        );
    }

    #[test]
    fn the_marker_names_the_collection_and_nothing_else() {
        let template = mark(
            ResourceTemplate::new("shop://items/{item_id}", "Item"),
            &collection("shop.items"),
        );
        assert_eq!(
            serde_json::to_value(&template).unwrap()["_meta"],
            serde_json::json!({"ai.veoveo/knowledge": {"collection": "shop.items"}})
        );
        let meta = &template.meta.unwrap().0;
        assert_eq!(
            CollectionMarker::from_meta(meta)
                .unwrap()
                .unwrap()
                .collection,
            collection("shop.items")
        );
    }

    #[test]
    fn the_descriptor_publishes_the_contracts_schemas() {
        let tool = descriptor();
        assert_eq!(tool.name, TOOL);
        assert_eq!(tool.input_schema, input_schema());
        assert_eq!(tool.output_schema, Some(output_schema()));
        assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
        let query = KnowledgeQuery::new(ContentParts::one(ContentPart::text("hello")).unwrap());
        let instance = serde_json::to_value(&query).unwrap();
        let validator =
            jsonschema::validator_for(&serde_json::Value::Object((*input_schema()).clone()))
                .unwrap();
        assert!(validator.is_valid(&instance), "{instance}");
    }
}
