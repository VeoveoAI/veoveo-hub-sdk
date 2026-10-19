use veoveo_hub_sdk_fixture::contract::{
    ObservatoryDocument, ObservatoryResource, ObservatoryScope, ReadingId,
};
use veoveo_types::{ResourceAddress, ResourceUri, ScopeDefinition, ScopeName};

#[test]
fn independent_owned_addresses_and_scope_names_round_trip() {
    for resource in [
        ObservatoryResource::Docs,
        ObservatoryResource::Document(ObservatoryDocument::Design),
        ObservatoryResource::Contract,
        ObservatoryResource::Readings,
        ObservatoryResource::Reading(ReadingId::new("sensor-a").unwrap()),
    ] {
        assert_eq!(
            ObservatoryResource::parse(&resource.to_uri().unwrap()).unwrap(),
            resource
        );
    }
    assert_eq!(ObservatoryScope::Read.name().as_str(), "observatory:read");
    assert!(ObservatoryScope::try_from(&ScopeName::parse("unrelated:read").unwrap()).is_err());
}

#[test]
fn owned_parser_rejects_wrong_routes_ids_and_parameters() {
    for value in [
        "other://readings",
        "observatory://reading",
        "observatory://reading/a/b",
        "observatory://reading/%61",
        "observatory://reading/A",
        "observatory://readings?cursor=bad",
        "observatory://reading/sensor-a#bad",
        "observatory://reading/{reading_id}",
    ] {
        // The URI profile rejects some spellings before the owner parser sees them.
        assert!(
            ResourceUri::new(value).map_or(true, |uri| ObservatoryResource::parse(&uri).is_err()),
            "{value}"
        );
    }
}

#[test]
fn shared_collection_selection_owns_uri_paging_and_default_tool_shape() {
    use veoveo_hub_sdk::{
        calls::ResourceRead,
        definition::{CollectionRequest, List, ReadCollection},
    };
    use veoveo_hub_sdk_fixture::contract::{ReadReadings, ReadingCollection, ReadingsTool};
    use veoveo_types::PageSize;
    let selection: List<ReadingsTool> =
        serde_json::from_value(serde_json::json!({"filters":{}})).unwrap();
    assert_eq!(selection.selection().limit().get(), 2);
    let read = ReadCollection::new(ReadReadings {}, PageSize::new(1).unwrap());
    assert_eq!(
        read.uri().unwrap().as_str(),
        "observatory://readings?limit=1"
    );
    for invalid in [
        "observatory://readings?limit=0",
        "observatory://readings?limit=3",
        "observatory://readings?cursor=",
        "observatory://readings?cursor=a&cursor=b",
        "observatory://readings?%63ursor=a&cursor=b",
    ] {
        let refused = ResourceUri::new(invalid).map_or(true, |uri| {
            CollectionRequest::<ReadingCollection>::split_uri(&uri, PageSize::new(2).unwrap())
                .is_err()
        });
        assert!(refused, "{invalid}");
    }
    assert!(
        serde_json::from_value::<List<ReadingsTool>>(serde_json::json!({"filters":{},"limit":3}))
            .is_err()
    );
}

#[cfg(feature = "server")]
#[tokio::test]
async fn resource_and_tool_pages_use_the_same_native_traversal() {
    use veoveo_hub_sdk::hosting::testing::{self, TestHub};
    use veoveo_hub_sdk_fixture::mcp::{self, ObservatoryMcp};
    let hub = TestHub::new(
        testing::for_domain::<ObservatoryMcp>()
            .handler(mcp::hosted)
            .build(),
    );
    let templates = hub
        .rpc("resources/templates/list", serde_json::json!({}))
        .await;
    assert!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|template| template["uriTemplate"] == "observatory://readings{?limit,cursor}")
    );
    for tool in [false, true] {
        let mut cursor: Option<String> = None;
        let mut ids = vec![];
        let mut pages = 0;
        loop {
            pages += 1;
            assert!(pages <= 3);
            let response = if tool {
                let mut arguments = serde_json::json!({"filters":{},"limit":2});
                if let Some(cursor) = &cursor {
                    arguments["cursor"] = serde_json::Value::String(cursor.clone());
                }
                hub.rpc(
                    "tools/call",
                    serde_json::json!({"name":"readings","arguments":arguments}),
                )
                .await
            } else {
                let mut uri = url::Url::parse("observatory://readings?limit=2").unwrap();
                if let Some(cursor) = &cursor {
                    uri.query_pairs_mut().append_pair("cursor", cursor);
                }
                hub.rpc("resources/read", serde_json::json!({"uri":uri.as_str()}))
                    .await
            };
            let page = if tool {
                response["result"]["structuredContent"].clone()
            } else {
                serde_json::from_str(response["result"]["contents"][0]["text"].as_str().unwrap())
                    .unwrap()
            };
            assert_eq!(page["limit"], 2, "{response}");
            ids.extend(
                page["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item["id"].as_str().unwrap().to_owned()),
            );
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(pages, 2);
        assert_eq!(ids, ["sensor-a", "sensor-b", "sensor-c"]);
    }
}
