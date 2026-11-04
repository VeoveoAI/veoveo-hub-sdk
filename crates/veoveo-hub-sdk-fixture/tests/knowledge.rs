//! Observatory expresses the knowledge contract: the SDK's one knowledge test over its own
//! fixture, whose readings and search are synthetic and in memory.
use serde_json::json;
use veoveo_hub_sdk::{
    hosting::testing::{self, TestHub},
    knowledge::{EXTENSION_ID, Modality},
};
use veoveo_hub_sdk_fixture::{
    contract::reading_knowledge,
    mcp::{self, ObservatoryMcp},
};

fn hub() -> TestHub {
    TestHub::new(
        testing::for_domain::<ObservatoryMcp>()
            .handler(mcp::hosted)
            .build(),
    )
}

#[tokio::test]
async fn observatory_answers_a_query_in_every_modality_with_readings_it_reads_back() {
    let hub = hub();
    let answers = veoveo_hub_sdk::testing::knowledge(&hub, "sensor").await;
    assert_eq!(answers.len(), Modality::ALL.len());
    for (modality, results) in &answers {
        let uris: Vec<&str> = results.hits().iter().map(|hit| hit.uri.as_str()).collect();
        assert_eq!(
            uris,
            [
                "observatory://reading/sensor-a",
                "observatory://reading/sensor-b",
                "observatory://reading/sensor-c"
            ],
            "{modality:?}"
        );
        assert!(
            results
                .hits()
                .iter()
                .all(|hit| hit.collection == reading_knowledge())
        );
    }
    assert_eq!(
        answers[&Modality::Text].hits()[0].snippet.as_str(),
        "sensor-a reads 7"
    );
}

#[tokio::test]
async fn the_reading_template_carries_its_collection_marker() {
    let hub = hub();
    let templates = hub.rpc("resources/templates/list", json!({})).await;
    let reading = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|template| template["uriTemplate"] == "observatory://reading/{reading_id}")
        .cloned()
        .unwrap_or_else(|| panic!("{templates}"));
    assert_eq!(
        reading["_meta"],
        json!({EXTENSION_ID: {"collection": "observatory.readings"}})
    );
    let narrowed = hub
        .rpc(
            "tools/call",
            json!({"name": "search_knowledge", "arguments": {
                "parts": [{"type": "text", "text": "sensor-b"}]
            }}),
        )
        .await;
    assert_eq!(
        narrowed["result"]["structuredContent"]["hits"][0]["uri"], "observatory://reading/sensor-b",
        "{narrowed}"
    );
    assert_eq!(
        narrowed["result"]["structuredContent"]["hits"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}
