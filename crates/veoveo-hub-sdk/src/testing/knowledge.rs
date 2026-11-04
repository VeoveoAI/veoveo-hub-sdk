//! The one test a server runs over its own fixture to show it expresses the knowledge
//! contract, as a client sees it through the server's MCP endpoint.
use std::collections::BTreeMap;

use serde_json::{Value, json};
use veoveo_types::{ResourceTemplateUri, ResourceUri};

use crate::{
    hosting::testing::TestHub,
    knowledge::{
        CollectionMarker, CollectionSelection, ContentPart, ContentParts, EXTENSION_ID, HeaderName,
        HeaderValue, KnowledgeCollection, KnowledgeLimit, KnowledgeQuery, KnowledgeResults,
        MediaHeaders, MediaSize, MediaType, Modality, SignedMedia, SignedUrl, TOOL, input_schema,
        output_schema,
    },
    registration::authorization_template::AuthorizationTemplate,
};

/// The signature every synthetic URL carries, which no answer may echo.
pub const SYNTHETIC_SIGNATURE: &str = "synthetic-signature";

/// The media a query of `modality` carries in [`knowledge`]: synthetic fixture data at
/// `https://media.veoveo.test/synthetic/{modality}`, `image/png`, `audio/wav` or
/// `video/mp4` of 1024 bytes, with one signed header. Nothing serves it, so a server's
/// fixture answers it without fetching, such as with a fake embedder; `None` for text.
pub fn synthetic_media(modality: Modality) -> Option<SignedMedia> {
    let media_type = match modality {
        Modality::Text => return None,
        Modality::Image => "image/png",
        Modality::Audio => "audio/wav",
        Modality::Video => "video/mp4",
    };
    let url = format!(
        "https://media.veoveo.test/synthetic/{}?X-Amz-Signature={SYNTHETIC_SIGNATURE}",
        modality.as_str()
    );
    Some(SignedMedia {
        url: SignedUrl::parse(url).expect("a synthetic signed URL"),
        headers: MediaHeaders::new([(
            HeaderName::parse("x-amz-security-token").expect("a header name"),
            HeaderValue::parse(SYNTHETIC_SIGNATURE).expect("a header value"),
        )])
        .expect("one header"),
        media_type: MediaType::parse(media_type).expect("a media type"),
        size: MediaSize::new(1024).expect("a media size"),
    })
}

/// Checks, through `hub`, that the server expresses the knowledge contract, and returns
/// the results of its query in each modality.
///
/// The server declares `ai.veoveo/knowledge` in `server/discover`, marks at least one
/// template, and publishes `search_knowledge` with the contract's schemas. A query of
/// `text`, and one each of an image, audio and a video of [`synthetic_media`], is
/// answered rather than refused, and the text query finds at least one record, so
/// `text` must match the fixture's records. Every hit passes the checks a client relies
/// on: at most the query's limit, an address of a template marked with its collection,
/// that collection one the query named when it named any, one resource link per hit,
/// and no echo of a signed URL. Each hit reads back through the server for the same
/// caller, `hub`'s test user, so `hub` grants what the search and the reads require.
/// The text query is repeated within each marked collection and with a limit of one.
pub async fn knowledge(hub: &TestHub, text: &str) -> BTreeMap<Modality, KnowledgeResults> {
    let discover = hub.rpc("server/discover", json!({})).await;
    assert!(
        discover["result"]["capabilities"]["extensions"][EXTENSION_ID].is_object(),
        "the server declares {EXTENSION_ID} in server/discover: {discover}"
    );
    let marked = marked_templates(hub).await;
    assert!(
        !marked.is_empty(),
        "the server marks at least one template as a knowledge collection"
    );
    let tools = list(hub, "tools/list", "tools").await;
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == TOOL)
        .unwrap_or_else(|| panic!("the server publishes {TOOL}"));
    assert_eq!(
        tool["inputSchema"],
        Value::Object((*input_schema()).clone()),
        "{TOOL} publishes the contract's input schema"
    );
    assert_eq!(
        tool["outputSchema"],
        Value::Object((*output_schema()).clone()),
        "{TOOL} publishes the contract's output schema"
    );
    let mut answers = BTreeMap::new();
    for modality in Modality::ALL {
        let part = match synthetic_media(modality) {
            None => ContentPart::text(text),
            Some(media) => ContentPart::media(modality, media).expect("a synthetic part"),
        };
        let query = KnowledgeQuery::new(ContentParts::one(part).expect("one part"));
        let results = search(hub, &marked, &query, modality).await;
        if modality == Modality::Text {
            assert!(
                !results.hits().is_empty(),
                "a text query of {text:?} finds at least one record of the fixture"
            );
        }
        answers.insert(modality, results);
    }
    let parts = ContentParts::one(ContentPart::text(text)).expect("one part");
    for (collection, _) in &marked {
        let query = KnowledgeQuery {
            collections: CollectionSelection::one(collection.clone()),
            ..KnowledgeQuery::new(parts.clone())
        };
        search(hub, &marked, &query, Modality::Text).await;
    }
    let one = KnowledgeQuery {
        limit: KnowledgeLimit::new(1).expect("a limit of one"),
        ..KnowledgeQuery::new(parts)
    };
    search(hub, &marked, &one, Modality::Text).await;
    answers
}

/// Each marked template's collection and template, from `resources/templates/list`.
async fn marked_templates(hub: &TestHub) -> Vec<(KnowledgeCollection, ResourceTemplateUri)> {
    let mut marked = Vec::new();
    for template in list(hub, "resources/templates/list", "resourceTemplates").await {
        let Some(meta) = template["_meta"].as_object() else {
            continue;
        };
        let Some(marker) = CollectionMarker::from_meta(meta) else {
            continue;
        };
        let marker = marker
            .unwrap_or_else(|error| panic!("a template's marker is a CollectionMarker: {error}"));
        let uri = template["uriTemplate"]
            .as_str()
            .and_then(|uri| ResourceTemplateUri::new(uri).ok())
            .unwrap_or_else(|| panic!("a marked template has a URI template: {template}"));
        assert!(
            marked
                .iter()
                .all(|(collection, _)| *collection != marker.collection),
            "the collection {} marks one template",
            marker.collection
        );
        marked.push((marker.collection, uri));
    }
    marked
}

/// Every item of a paged list method.
async fn list(hub: &TestHub, method: &str, key: &str) -> Vec<Value> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..64 {
        let params = match &cursor {
            Some(cursor) => json!({"cursor": cursor}),
            None => json!({}),
        };
        let page = hub.rpc(method, params).await;
        let entries = page["result"][key]
            .as_array()
            .unwrap_or_else(|| panic!("{method} answers: {page}"));
        items.extend(entries.iter().cloned());
        match page["result"]["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return items,
        }
    }
    panic!("{method} ends within 64 pages")
}

/// Calls the search with `query`, checks its answer and reads each hit back.
async fn search(
    hub: &TestHub,
    marked: &[(KnowledgeCollection, ResourceTemplateUri)],
    query: &KnowledgeQuery,
    modality: Modality,
) -> KnowledgeResults {
    let arguments = serde_json::to_value(query).expect("a query serializes");
    let answer = hub
        .rpc("tools/call", json!({"name": TOOL, "arguments": arguments}))
        .await;
    let text = answer.to_string();
    assert!(
        !text.contains(SYNTHETIC_SIGNATURE) && !text.contains("media.veoveo.test"),
        "a {modality:?} query's answer never echoes its signed URL or headers"
    );
    let result = &answer["result"];
    assert!(
        result.is_object() && result["isError"] != true,
        "a {modality:?} query is answered rather than refused: {answer}"
    );
    let results: KnowledgeResults = serde_json::from_value(result["structuredContent"].clone())
        .unwrap_or_else(|error| panic!("a {modality:?} query answers KnowledgeResults: {error}"));
    let limit = query.limit.get() as usize;
    assert!(
        results.hits().len() <= limit,
        "a {modality:?} query answers at most its limit of {limit} hits"
    );
    let links: Vec<&str> = result["content"]
        .as_array()
        .map(|content| {
            content
                .iter()
                .filter(|item| item["type"] == "resource_link")
                .filter_map(|item| item["uri"].as_str())
                .collect()
        })
        .unwrap_or_default();
    let uris: Vec<&str> = results.hits().iter().map(|hit| hit.uri.as_str()).collect();
    assert_eq!(links, uris, "the answer holds one resource link per hit");
    for hit in results.hits() {
        let template = marked
            .iter()
            .find(|(collection, _)| *collection == hit.collection)
            .map(|(_, template)| template)
            .unwrap_or_else(|| {
                panic!(
                    "the hit {} names {}, a marked collection",
                    hit.uri, hit.collection
                )
            });
        assert!(
            addresses(template, &hit.uri),
            "the hit {} is an address of {template}",
            hit.uri
        );
        assert!(
            query.searches(&hit.collection),
            "the hit {} is of a collection the query names",
            hit.uri
        );
        let read = hub.rpc("resources/read", json!({"uri": hit.uri})).await;
        assert!(
            read["result"]["contents"]
                .as_array()
                .is_some_and(|contents| !contents.is_empty()),
            "the hit {} reads back through the server for the same caller: {read}",
            hit.uri
        );
    }
    results
}

fn addresses(template: &ResourceTemplateUri, uri: &ResourceUri) -> bool {
    AuthorizationTemplate::parse(template).is_some_and(|template| template.matches(uri))
}
