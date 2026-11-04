use serde_json::{Value, json};
use veoveo_types::ServerSlug;

use super::*;

/// A URL whose query carries a signature, which nothing may echo.
const SIGNED: &str = "https://storage.veoveo.test/media/a.png?X-Amz-Signature=sig-0123456789";
const SECRET_HEADER: &str = "session-token-0123456789";

fn media(media_type: &str) -> SignedMedia {
    SignedMedia {
        url: SignedUrl::parse(SIGNED).unwrap(),
        headers: MediaHeaders::new([(
            HeaderName::parse("x-amz-security-token").unwrap(),
            HeaderValue::parse(SECRET_HEADER).unwrap(),
        )])
        .unwrap(),
        media_type: MediaType::parse(media_type).unwrap(),
        size: MediaSize::new(1024).unwrap(),
    }
}

fn image_part() -> Value {
    json!({
        "type": "image",
        "url": SIGNED,
        "headers": {"x-amz-security-token": SECRET_HEADER},
        "mediaType": "image/png",
        "size": 1024
    })
}

fn query(value: Value) -> Result<KnowledgeQuery, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn collections_are_a_server_slug_and_a_plural() {
    let collection = KnowledgeCollection::parse("artifact.artifacts").unwrap();
    assert_eq!(collection.server(), ServerSlug::parse("artifact").unwrap());
    assert_eq!(collection.plural(), "artifacts");
    assert!(collection.is_owned_by(&ServerSlug::parse("artifact").unwrap()));
    assert!(!collection.is_owned_by(&ServerSlug::parse("artifacts").unwrap()));
    assert_eq!(
        KnowledgeCollection::of(&ServerSlug::parse("time").unwrap(), "events").unwrap(),
        KnowledgeCollection::parse("time.events").unwrap()
    );
    assert!(KnowledgeCollection::parse("time-zones.zone_names").is_ok());
    for invalid in [
        "",
        "artifact",
        "artifact.",
        ".artifacts",
        "artifact.artifacts.read",
        "Artifact.artifacts",
        "artifact.Artifacts",
        "artifact.1artifacts",
        "art__ifact.artifacts",
        "artifact.art ifacts",
        "artifact:artifacts",
    ] {
        assert!(KnowledgeCollection::parse(invalid).is_err(), "{invalid}");
    }
    assert!(KnowledgeCollection::parse(format!("a.{}", "b".repeat(126))).is_ok());
    assert!(KnowledgeCollection::parse(format!("a.{}", "b".repeat(127))).is_err());
    assert!(serde_json::from_value::<KnowledgeCollection>(json!("artifact")).is_err());
}

#[test]
fn a_marker_holds_its_collection_alone() {
    let marker = CollectionMarker {
        collection: KnowledgeCollection::parse("time.events").unwrap(),
    };
    assert_eq!(marker.to_value(), json!({"collection": "time.events"}));
    let mut meta = serde_json::Map::new();
    assert!(CollectionMarker::from_meta(&meta).is_none());
    meta.insert(EXTENSION_ID.into(), marker.to_value());
    assert_eq!(CollectionMarker::from_meta(&meta).unwrap().unwrap(), marker);
    meta.insert(
        EXTENSION_ID.into(),
        json!({"collection": "time.events", "kind": "events"}),
    );
    assert!(CollectionMarker::from_meta(&meta).unwrap().is_err());
}

#[test]
fn a_query_holds_1_to_16_parts_and_a_limit_of_1_to_20() {
    let text = json!({"type": "text", "text": "a red kite"});
    let admitted = query(json!({"parts": [text]})).unwrap();
    assert_eq!(admitted.limit.get(), DEFAULT_LIMIT);
    assert!(admitted.collections.is_empty());
    assert_eq!(admitted.parts.as_slice(), [ContentPart::text("a red kite")]);
    let sixteen = vec![text.clone(); MAX_PARTS];
    assert_eq!(
        query(json!({"parts": sixteen}))
            .unwrap()
            .parts
            .as_slice()
            .len(),
        16
    );
    for limit in [1, 20] {
        assert_eq!(
            query(json!({"parts": [text], "limit": limit}))
                .unwrap()
                .limit
                .get(),
            limit
        );
    }
    let seventeen = vec![text.clone(); MAX_PARTS + 1];
    for refused in [
        json!({"parts": []}),
        json!({"parts": seventeen}),
        json!({}),
        json!({"parts": [text], "limit": 0}),
        json!({"parts": [text], "limit": 21}),
        json!({"parts": [text], "limit": null}),
        json!({"parts": [text], "collections": ["artifact"]}),
        json!({"parts": [text], "unknown": true}),
        json!({"parts": [{"type": "text", "text": "x", "url": SIGNED}]}),
        json!({"parts": [{"type": "document", "text": "x"}]}),
    ] {
        assert!(query(refused.clone()).is_err(), "{refused}");
    }
    let named = query(json!({"parts": [text], "collections": ["time.events"]})).unwrap();
    assert_eq!(
        named.collections.as_slice(),
        [KnowledgeCollection::parse("time.events").unwrap()]
    );
    assert!(named.searches(&KnowledgeCollection::parse("time.events").unwrap()));
    assert!(!named.searches(&KnowledgeCollection::parse("time.calendars").unwrap()));
    assert!(admitted.searches(&KnowledgeCollection::parse("time.calendars").unwrap()));
}

/// `count` distinct collections of the server `time`.
fn distinct_collections(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("time.c{index}")).collect()
}

#[test]
fn a_query_names_at_most_16_collections_each_once() {
    let text = json!({"type": "text", "text": "a red kite"});
    let sixteen = distinct_collections(MAX_QUERY_COLLECTIONS);
    let admitted = query(json!({"parts": [text], "collections": sixteen})).unwrap();
    assert_eq!(admitted.collections.as_slice().len(), 16);
    let seventeen = distinct_collections(MAX_QUERY_COLLECTIONS + 1);
    let refused = query(json!({"parts": [text], "collections": seventeen})).unwrap_err();
    assert!(refused.to_string().contains("at most 16"), "{refused}");
    let duplicate =
        json!({"parts": [text], "collections": ["time.events", "time.calendars", "time.events"]});
    let refused = query(duplicate).unwrap_err();
    assert!(refused.to_string().contains("once"), "{refused}");
    let parse = |names: Vec<String>| {
        CollectionSelection::new(
            names
                .iter()
                .map(|name| KnowledgeCollection::parse(name).unwrap())
                .collect(),
        )
    };
    assert!(parse(distinct_collections(16)).is_ok());
    assert!(parse(distinct_collections(17)).is_err());
    assert!(parse(vec!["time.events".into(), "time.events".into()]).is_err());
    assert!(parse(Vec::new()).unwrap().is_empty());
    let omitted = serde_json::to_value(KnowledgeQuery::new(admitted.parts.clone())).unwrap();
    assert!(omitted.get("collections").is_none(), "{omitted}");
}

#[test]
fn a_media_part_carries_signed_media_of_its_own_modality() {
    let admitted = query(json!({"parts": [image_part()]})).unwrap();
    let part = &admitted.parts.as_slice()[0];
    assert_eq!(part.modality(), Modality::Image);
    let signed = part.signed_media().unwrap();
    assert_eq!(signed.url.expose(), SIGNED);
    assert_eq!(signed.media_type.as_str(), "image/png");
    assert_eq!(signed.size.bytes(), 1024);
    assert_eq!(serde_json::to_value(part).unwrap(), image_part());
    // Audio may be a video's soundtrack.
    for (kind, media_type, admitted) in [
        ("image", "image/png", true),
        ("image", "video/mp4", false),
        ("audio", "audio/wav", true),
        ("audio", "video/mp4", true),
        ("audio", "image/png", false),
        ("video", "video/mp4", true),
        ("video", "audio/wav", false),
    ] {
        let mut part = image_part();
        part["type"] = json!(kind);
        part["mediaType"] = json!(media_type);
        assert_eq!(
            query(json!({"parts": [part]})).is_ok(),
            admitted,
            "{kind} {media_type}"
        );
        let modality = serde_json::from_value::<Modality>(json!(kind)).unwrap();
        assert_eq!(
            ContentPart::media(modality, media(media_type)).is_ok(),
            admitted
        );
    }
    assert!(ContentPart::media(Modality::Text, media("image/png")).is_err());
    let unsigned = json!({"type": "image", "url": SIGNED, "mediaType": "image/png", "size": 1});
    assert!(query(json!({"parts": [unsigned]})).is_ok());
    for (pointer, value) in [
        ("/url", json!("ftp://storage.veoveo.test/a.png")),
        ("/url", json!("/media/a.png")),
        ("/url", json!("https://storage veoveo.test/a.png")),
        (
            "/url",
            json!(format!("https://s.test/{}", "a".repeat(8192))),
        ),
        ("/mediaType", json!("image/PNG")),
        ("/mediaType", json!("image/png; charset=binary")),
        ("/mediaType", json!("image")),
        ("/size", json!(0)),
        ("/size", json!(MAX_MEDIA_BYTES + 1)),
        ("/headers", json!({"bad name": "x"})),
        ("/headers", json!({"x-ok": "line\nbreak"})),
        ("/headers", json!({"x-ok": 5})),
    ] {
        let mut part = image_part();
        *part.pointer_mut(pointer).unwrap() = value.clone();
        assert!(
            query(json!({"parts": [part]})).is_err(),
            "{pointer} {value}"
        );
    }
    let mut extra = image_part();
    extra["expiresAt"] = json!("2026-11-03T00:00:00Z");
    assert!(query(json!({"parts": [extra]})).is_err());
    let many: serde_json::Map<String, Value> = (0..=MAX_MEDIA_HEADERS)
        .map(|index| (format!("x-h{index}"), json!("v")))
        .collect();
    let mut crowded = image_part();
    crowded["headers"] = Value::Object(many);
    assert!(query(json!({"parts": [crowded]})).is_err());
}

#[test]
fn a_signed_url_and_its_headers_never_reach_debug_output_or_errors() {
    let admitted = query(json!({"parts": [image_part()]})).unwrap();
    let debug = format!("{admitted:?} {:#?}", media("image/png"));
    assert!(!debug.contains("X-Amz-Signature"), "{debug}");
    assert!(!debug.contains("storage.veoveo.test"), "{debug}");
    assert!(!debug.contains(SECRET_HEADER), "{debug}");
    assert!(debug.contains("SignedUrl([REDACTED])"), "{debug}");
    assert!(debug.contains("image/png"), "{debug}");
    assert_eq!(
        format!("{:?}", HeaderValue::parse(SECRET_HEADER).unwrap()),
        "HeaderValue([REDACTED])"
    );
    // Every refusal names the rule it broke, never the value.
    let long = format!("{SIGNED}&pad={}", "a".repeat(MAX_SIGNED_URL_BYTES));
    let spaced = format!("{SIGNED} trailing");
    for refused in [long.as_str(), spaced.as_str()] {
        let error = SignedUrl::parse(refused).unwrap_err();
        assert!(!error.to_string().contains("sig-0123456789"), "{error}");
        let mut part = image_part();
        part["url"] = json!(refused);
        let error = query(json!({"parts": [part]})).unwrap_err().to_string();
        assert!(!error.contains("sig-0123456789"), "{error}");
    }
    let mut mismatched = image_part();
    mismatched["mediaType"] = json!("audio/wav");
    let error = query(json!({"parts": [mismatched]}))
        .unwrap_err()
        .to_string();
    assert!(!error.contains("sig-0123456789"), "{error}");
    let header = HeaderValue::parse(format!("{SECRET_HEADER}\r\n")).unwrap_err();
    assert!(!header.to_string().contains(SECRET_HEADER), "{header}");
}

#[test]
fn snippets_are_trimmed_to_320_characters_and_refused_beyond() {
    let long = "é".repeat(400);
    let snippet = Snippet::new(long.clone());
    assert_eq!(snippet.as_str().chars().count(), MAX_SNIPPET_CHARS);
    assert_eq!(Snippet::new("short").as_str(), "short");
    let exact = "a".repeat(MAX_SNIPPET_CHARS);
    assert_eq!(Snippet::new(exact.clone()).as_str(), exact);
    assert!(serde_json::from_value::<Snippet>(json!(exact)).is_ok());
    assert!(serde_json::from_value::<Snippet>(json!(long)).is_err());
}

#[test]
fn results_hold_at_most_20_hits_in_rank_order() {
    let hit = |index: usize| KnowledgeHit {
        uri: ResourceUri::new(format!("time://events/{index}")).unwrap(),
        collection: KnowledgeCollection::parse("time.events").unwrap(),
        title: format!("Event {index}"),
        snippet: Snippet::new("An event"),
        modified_at: (index == 0).then(|| "2026-11-03T10:00:00Z".parse().unwrap()),
    };
    let results = KnowledgeResults::new((0..20).map(hit).collect()).unwrap();
    let wire = serde_json::to_value(&results).unwrap();
    assert_eq!(wire["hits"][0]["modifiedAt"], "2026-11-03T10:00:00Z");
    assert!(wire["hits"][1].get("modifiedAt").is_none());
    assert_eq!(wire["hits"][19]["uri"], "time://events/19");
    assert_eq!(
        serde_json::from_value::<KnowledgeResults>(wire).unwrap(),
        results
    );
    assert!(KnowledgeResults::new((0..21).map(hit).collect()).is_err());
    let crowded = json!({"hits": (0..21).map(|index| serde_json::to_value(hit(index)).unwrap()).collect::<Vec<_>>()});
    assert!(serde_json::from_value::<KnowledgeResults>(crowded).is_err());
    let mut unknown = serde_json::to_value(hit(1)).unwrap();
    unknown["score"] = json!(0.5);
    assert!(serde_json::from_value::<KnowledgeHit>(unknown).is_err());
}

#[test]
fn embeddings_hold_one_vector_of_the_models_dimensions_per_item() {
    let model = EmbeddingModel {
        checkpoint: ModelCheckpoint::parse("google/embeddinggemma-300m@64614b0b").unwrap(),
        dimensions: EmbeddingDimensions::new(3).unwrap(),
    };
    let embeddings = Embeddings::new(model.clone(), vec![vec![0.1, 0.2, 0.3]; 2]).unwrap();
    let wire = serde_json::to_value(&embeddings).unwrap();
    assert_eq!(
        wire["model"],
        json!({"checkpoint": "google/embeddinggemma-300m@64614b0b", "dimensions": 3})
    );
    assert_eq!(
        serde_json::from_value::<Embeddings>(wire).unwrap(),
        embeddings
    );
    assert_eq!(embeddings.vectors().len(), 2);
    assert!(Embeddings::new(model.clone(), vec![vec![0.1, 0.2]]).is_err());
    assert!(Embeddings::new(model.clone(), vec![vec![0.1, f32::NAN, 0.3]]).is_err());
    assert!(Embeddings::new(model, Vec::new()).is_ok());
    assert!(
        serde_json::from_value::<Embeddings>(json!({
            "model": {"checkpoint": "m", "dimensions": 2},
            "vectors": [[0.1, 0.2, 0.3]]
        }))
        .is_err()
    );
    assert!(EmbeddingDimensions::new(0).is_none());
    assert!(ModelCheckpoint::parse("a model").is_err());
    assert!(ModelCheckpoint::parse("m".repeat(MAX_CHECKPOINT_BYTES + 1)).is_err());
    assert_eq!(
        serde_json::to_value(EmbeddingPurpose::Query).unwrap(),
        json!("query")
    );
}

/// C07: the published input schema closes every object, gives each argument one JSON
/// type, omits rather than nulls an optional argument, declares `type` on every
/// restricted subschema and admits exactly the contract's queries.
#[cfg(feature = "server")]
#[test]
fn the_published_input_schema_satisfies_the_tool_schema_profile() {
    let schema = Value::Object((*input_schema()).clone());
    assert!(
        crate::hosting::undeclared_types(&schema).is_empty(),
        "{:?}",
        crate::hosting::undeclared_types(&schema)
    );
    assert!(crate::hosting::ambiguous_arguments(&schema).is_empty());
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["required"], json!(["parts"]));
    for argument in ["parts", "collections", "limit"] {
        let declared = &schema["properties"][argument]["type"];
        assert!(declared.is_string(), "{argument}: {declared}");
    }
    assert_eq!(schema["properties"]["limit"]["default"], DEFAULT_LIMIT);
    assert_eq!(
        schema["properties"]["collections"]["maxItems"],
        MAX_QUERY_COLLECTIONS
    );
    assert_eq!(schema["properties"]["collections"]["uniqueItems"], true);
    let mut objects = Vec::new();
    closed_objects(&schema, String::new(), &mut objects);
    assert!(!objects.is_empty());
    for (pointer, closed) in objects {
        assert!(closed, "an open object at {pointer}: {schema}");
    }
    let validator = jsonschema::validator_for(&schema).unwrap();
    for admitted in [
        json!({"parts": [{"type": "text", "text": "kite"}]}),
        json!({"parts": [image_part()], "collections": ["time.events"], "limit": 20}),
    ] {
        assert!(validator.is_valid(&admitted), "{admitted}");
        assert!(query(admitted).is_ok());
    }
    let seventeen = vec![json!({"type": "text", "text": "x"}); MAX_PARTS + 1];
    let crowded = distinct_collections(MAX_QUERY_COLLECTIONS + 1);
    for refused in [
        json!({"parts": []}),
        json!({"parts": seventeen}),
        json!({"parts": [{"type": "text", "text": "x"}], "collections": crowded}),
        json!({"parts": [{"type": "text", "text": "x"}], "collections": ["time.events", "time.events"]}),
        json!({"parts": [{"type": "text", "text": "x"}], "limit": 21}),
        json!({"parts": [{"type": "text", "text": "x"}], "limit": null}),
        json!({"parts": [{"type": "text", "text": "x"}], "other": 1}),
        json!({"parts": [{"type": "image", "url": SIGNED, "mediaType": "image/png", "size": 1, "x": 1}]}),
        json!({"parts": [{"type": "image", "url": SIGNED, "mediaType": "IMAGE/PNG", "size": 1}]}),
        json!({"parts": [{"type": "image", "url": SIGNED, "mediaType": "image/png", "size": 0}]}),
    ] {
        assert!(!validator.is_valid(&refused), "{refused}");
        assert!(query(refused).is_err());
    }
    let output = Value::Object((*output_schema()).clone());
    assert_eq!(output["type"], "object");
    assert_eq!(output["properties"]["hits"]["maxItems"], MAX_HITS);
}

/// Every object subschema of `schema` and whether it closes its members: an object that
/// declares properties refuses others, a dictionary types its values, and a reference or
/// composition leaves closing to the subschemas it names, which the walk also reaches.
#[cfg(feature = "server")]
fn closed_objects(schema: &Value, pointer: String, found: &mut Vec<(String, bool)>) {
    match schema {
        Value::Object(object) => {
            if object.get("type") == Some(&json!("object")) {
                let composed = ["$ref", "oneOf", "anyOf", "allOf"]
                    .iter()
                    .any(|keyword| object.contains_key(*keyword));
                let closed = composed
                    || match object.get("additionalProperties") {
                        Some(Value::Bool(false)) => true,
                        Some(Value::Object(values)) => values.contains_key("type"),
                        _ => false,
                    };
                found.push((pointer.clone(), closed));
            }
            for (key, value) in object {
                closed_objects(value, format!("{pointer}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                closed_objects(value, format!("{pointer}/{index}"), found);
            }
        }
        _ => {}
    }
}
