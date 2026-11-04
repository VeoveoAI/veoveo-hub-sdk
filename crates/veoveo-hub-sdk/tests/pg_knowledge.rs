//! The ready-made PostgreSQL knowledge search on PostgreSQL 18.6 with pgvector 0.8.7, on
//! a private cluster from core's store fixture. Every embedding here is synthetic: a
//! fixed word-count vector, never a model's, so the tests prove ranking, filtering and
//! writes, not retrieval quality. Writes embed through a probe server in the in-process
//! mesh standing in for Embedding, which records who each call came from.
#![cfg(feature = "testing")]
#[path = "support/pg_knowledge.rs"]
mod support;

use serde_json::{Value, json};
use support::{Change, Interleaving, Shop, Synthetic};
use uuid::Uuid;
use veoveo_hub_sdk::hosting::testing;
use veoveo_hub_sdk::knowledge::{
    ContentPart, ContentParts, INDEX_ATTEMPTS, Indexed, KnowledgeCollection, NotEmbedded,
    PgKnowledgeError, VECTOR_DIMENSIONS,
};

fn hits(answer: &Value) -> Vec<String> {
    answer["result"]["structuredContent"]["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"))
        .iter()
        .map(|hit| hit["uri"].as_str().unwrap().to_owned())
        .collect()
}

fn text(query: &str) -> Value {
    json!({"parts":[{"type":"text","text":query}]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_read_rule_applies_before_ranking_and_the_limit_so_a_page_stays_full() {
    let shop = Shop::start().await;
    // Fifty records the caller may not read rank above twelve it may: each says "kite"
    // three times. A rule applied after the limit would leave no page at all.
    for index in 0..50 {
        shop.item(&format!("hidden kite kite kite {index}"), false)
            .await;
    }
    let mut readable = Vec::new();
    for index in 0..12 {
        readable.push(shop.item(&format!("kite {index}"), true).await);
    }
    shop.index_all().await;
    let answer = shop
        .search(json!({"parts":[{"type":"text","text":"kite"}],"limit":10}))
        .await;
    let found = hits(&answer);
    assert_eq!(found.len(), 10, "{answer}");
    for uri in &found {
        assert!(
            readable
                .iter()
                .any(|key| uri == &format!("shop://items/{key}")),
            "{uri} is not readable by the caller"
        );
    }
    assert!(!answer.to_string().contains("hidden"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_and_media_queries_rank_and_vectors_of_another_model_are_never_compared() {
    let shop = Shop::start().await;
    let kite = shop.item("kite over the dunes", true).await;
    let river = shop.item("river at dawn", true).await;
    let image = shop.item("image of a kite", true).await;
    shop.index_all().await;
    // Another model's vector, equal to the media query's, on a record no words reach.
    let other = shop.item("nothing in common", true).await;
    shop.vector(other, &Synthetic::media_vector(), "other/model@1")
        .await;

    // Both words and vectors rank the two kites; the river only by its distant vector.
    let words = hits(&shop.search(text("kite")).await);
    let top: std::collections::BTreeSet<_> = words.iter().take(2).cloned().collect();
    assert_eq!(
        top,
        [
            format!("shop://items/{kite}"),
            format!("shop://items/{image}")
        ]
        .into_iter()
        .collect()
    );
    let river = format!("shop://items/{river}");
    assert!(
        words
            .iter()
            .position(|uri| *uri == river)
            .is_none_or(|at| at >= 2)
    );

    // A media-only query has no text ranking: its cosine ranking alone answers.
    let media = shop.search(Synthetic::image_query()).await;
    let found = hits(&media);
    assert!(!found.is_empty(), "{media}");
    assert!(!found.contains(&format!("shop://items/{other}")), "{media}");
    assert!(!media.to_string().contains("media.veoveo.test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sources_with_uuid_and_text_keys_fuse_into_one_ranking() {
    let shop = Shop::start().await;
    let item = shop.item("lantern festival", true).await;
    shop.note("note-lantern", "lantern repair notes", true)
        .await;
    shop.note("note-hidden", "lantern secret", false).await;
    shop.index_all().await;
    let answer = shop.search(text("lantern")).await;
    let found = hits(&answer);
    assert_eq!(
        found
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            format!("shop://items/{item}"),
            "shop://notes/note-lantern".to_owned()
        ]
        .into_iter()
        .collect(),
        "{answer}"
    );
    let collections: Vec<&str> = answer["result"]["structuredContent"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["collection"].as_str().unwrap())
        .collect();
    assert!(collections.contains(&"shop.items") && collections.contains(&"shop.notes"));
    let only_notes = shop
        .search(json!({"parts":[{"type":"text","text":"lantern"}],"collections":["shop.notes"]}))
        .await;
    assert_eq!(hits(&only_notes), ["shop://notes/note-lantern"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_admits_a_complete_table_and_refuses_each_missing_piece() {
    let shop = Shop::start().await;
    shop.knowledge().check(shop.pool()).await.unwrap();
    for (broken, expected) in [
        (
            "ALTER TABLE shop.item DROP COLUMN knowledge_model",
            "knowledge_model",
        ),
        (
            "ALTER TABLE shop.item ALTER COLUMN knowledge_vector TYPE _extensions.vector(3) USING NULL",
            "knowledge_vector",
        ),
        ("DROP INDEX shop.item_knowledge_vector", "HNSW"),
        ("DROP INDEX shop.item_knowledge_text", "GIN"),
        (
            "DROP FUNCTION shop.item_readable(shop.item, jsonb)",
            "read rule",
        ),
    ] {
        let shop = Shop::start().await;
        shop.execute(broken).await;
        let error = shop.knowledge().check(shop.pool()).await.unwrap_err();
        assert!(error.to_string().contains(expected), "{broken}: {error}");
        assert!(
            matches!(
                error,
                PgKnowledgeError::Column { .. }
                    | PgKnowledgeError::Index { .. }
                    | PgKnowledgeError::ReadRule { .. }
            ),
            "{broken}: {error:?}"
        );
    }
    // A text key the source declares as a UUID is refused too.
    let shop = Shop::start().await;
    shop.execute("ALTER TABLE shop.item ALTER COLUMN item_id TYPE text")
        .await;
    let error = shop.knowledge().check(shop.pool()).await.unwrap_err();
    assert!(error.to_string().contains("item_id is not uuid"), "{error}");
    // A volatile read rule cannot filter a ranking.
    let shop = Shop::start().await;
    shop.execute("ALTER FUNCTION shop.note_readable(shop.note, jsonb) VOLATILE")
        .await;
    assert!(matches!(
        shop.knowledge().check(shop.pool()).await,
        Err(PgKnowledgeError::ReadRule { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn index_writes_vectors_and_reports_a_failing_record_without_failing_the_others() {
    let shop = Shop::start().await;
    let good = shop.item("good record", true).await;
    let broken = shop.item("broken record", true).await;
    let missing = Uuid::now_v7();
    let other = shop.item("another good record", true).await;
    let outcomes = shop.index_items(&[good, broken, missing, other]).await;
    assert_eq!(
        outcomes,
        [
            (good, Indexed::Embedded),
            (
                broken,
                Indexed::NotEmbedded(NotEmbedded::Content("synthetic unreadable content".into()))
            ),
            (missing, Indexed::NotEmbedded(NotEmbedded::NoContent)),
            (other, Indexed::Embedded),
        ]
    );
    let (model, dimensions) = shop.stored(good).await;
    assert_eq!(model.as_deref(), Some(Synthetic::CHECKPOINT));
    assert_eq!(dimensions, Some(VECTOR_DIMENSIONS as i32));
    assert_eq!(shop.stored(broken).await, (None, None));
    // An Embedding call that fails reports it for each of its records.
    let failing = shop.item("fail-embedding please", true).await;
    let outcomes = shop.index_items(&[failing]).await;
    assert!(
        matches!(&outcomes[0].1, Indexed::NotEmbedded(NotEmbedded::Embedding(reason)) if reason.contains("synthetic")),
        "{outcomes:?}"
    );
    // A collection the search has no source for is refused.
    let unknown = KnowledgeCollection::parse("shop.orders").unwrap();
    assert!(matches!(
        shop.index_unknown(&unknown).await,
        Err(PgKnowledgeError::UnknownCollection(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_written_again_while_it_is_embedded_gets_its_latest_contents_vector() {
    let shop = Shop::start().await;
    // A write commits during the first Embedding call: that vector is of older content,
    // so it is not written, and the record is read and embedded again.
    let key = shop.item("first draft", true).await;
    let embedder = Interleaving::new(shop.pool(), key, Change::RenameOnce("final kite".into()));
    let outcomes = shop.index_with(embedder.clone(), &[key]).await;
    assert_eq!(outcomes, [(key, Indexed::Embedded)]);
    assert_eq!(embedder.texts(), ["first draft", "final kite"]);
    let latest = ContentParts::one(ContentPart::text("final kite")).unwrap();
    assert_eq!(
        shop.stored_vector(key).await,
        Some(Synthetic::vector(&latest))
    );

    // A record written during every attempt keeps no vector and says why.
    let restless = shop.item("restless", true).await;
    let embedder = Interleaving::new(shop.pool(), restless, Change::RenameAlways);
    let outcomes = shop.index_with(embedder.clone(), &[restless]).await;
    assert_eq!(
        outcomes,
        [(restless, Indexed::NotEmbedded(NotEmbedded::Changed))]
    );
    assert_eq!(embedder.texts().len(), INDEX_ATTEMPTS);
    assert_eq!(shop.stored(restless).await, (None, None));

    // A record deleted while it is embedded is gone, not failed.
    let doomed = shop.item("doomed", true).await;
    let embedder = Interleaving::new(shop.pool(), doomed, Change::Delete);
    let outcomes = shop.index_with(embedder, &[doomed]).await;
    assert_eq!(
        outcomes,
        [(doomed, Indexed::NotEmbedded(NotEmbedded::Gone))]
    );
}

fn related_task(answer: &Value) -> Option<&str> {
    answer["result"]["_meta"][veoveo_hub_sdk::RELATED_TASK_META_KEY]["taskId"].as_str()
}

fn written_key(written: &Value) -> Uuid {
    Uuid::parse_str(written["key"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_direct_write_names_a_task_that_embeds_the_record_as_the_caller() {
    let shop = Shop::start().await;
    let answer = shop.write("lantern written directly").await;
    let task = related_task(&answer).unwrap_or_else(|| panic!("{answer}"));
    let written = &answer["result"]["structuredContent"];
    assert_eq!(written["task"], task, "{answer}");
    assert_eq!(written["records"], json!([]));
    let key = written_key(written);

    // The person follows the Task the write named, as one of their own.
    let settled = shop.settled(task).await;
    assert_eq!(settled["result"]["status"], "completed", "{settled}");
    assert_eq!(
        shop.stored(key).await.0.as_deref(),
        Some(Synthetic::CHECKPOINT)
    );

    // Embedding was called for the caller, by the shop from that Task.
    let seen = shop.seen("lantern written directly");
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].caller, testing::principal().id.to_string());
    assert_eq!(seen[0].acting_server.as_deref(), Some("shop"));
    assert_eq!(seen[0].acting_task.as_deref(), Some(task));
    assert!(seen[0].prior_servers.is_empty(), "{seen:?}");
    assert_eq!(
        hits(&shop.search(text("lantern")).await),
        [format!("shop://items/{key}")]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_direct_write_whose_embedding_fails_fails_its_task_naming_the_record() {
    let shop = Shop::start().await;
    let answer = shop.write("fail-embedding written directly").await;
    let task = related_task(&answer).unwrap_or_else(|| panic!("{answer}"));
    let key = written_key(&answer["result"]["structuredContent"]);
    let settled = shop.settled(task).await;
    assert_eq!(settled["result"]["status"], "failed", "{settled}");
    let failure = settled.to_string();
    assert!(
        failure.contains("1 of 1 records could not be embedded")
            && failure.contains("knowledge_index")
            && failure.contains(&key.to_string())
            && failure.contains("synthetic embedding failure"),
        "{settled}"
    );
    assert_eq!(shop.stored(key).await, (None, None));
    assert_eq!(shop.seen("fail-embedding written directly").len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delegated_write_embeds_before_it_returns_continuing_the_chain_of_servers() {
    let shop = Shop::start().await;
    let answer = shop.write_through_desk("lantern written by the desk").await;
    let relayed = &answer["result"]["structuredContent"];
    // No Task: one another server's call created could not act for the caller.
    assert_eq!(relayed["relatedTask"], Value::Null, "{answer}");
    assert_eq!(related_task(&answer), None);
    let written = &relayed["written"];
    assert_eq!(written["task"], Value::Null, "{answer}");
    let key = written_key(written);
    assert_eq!(
        written["records"],
        json!([{"key": key.to_string(), "embedded": true}])
    );
    // Embedded by the time the desk had its answer.
    assert_eq!(
        shop.stored(key).await.0.as_deref(),
        Some(Synthetic::CHECKPOINT)
    );
    let seen = shop.seen("lantern written by the desk");
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].caller, testing::principal().id.to_string());
    assert_eq!(seen[0].acting_server.as_deref(), Some("shop"));
    assert_eq!(seen[0].acting_task, None);
    assert_eq!(seen[0].prior_servers, ["desk"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delegated_write_whose_embedding_fails_reports_it_in_the_outcome() {
    let shop = Shop::start().await;
    let answer = shop
        .write_through_desk("fail-embedding written by the desk")
        .await;
    let written = &answer["result"]["structuredContent"]["written"];
    let key = written_key(written);
    let record = &written["records"][0];
    assert_eq!(record["key"], key.to_string(), "{answer}");
    assert_eq!(record["embedded"], false, "{answer}");
    assert!(
        record["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("could not be embedded")
                && reason.contains("synthetic embedding failure")),
        "{answer}"
    );
    assert_eq!(shop.stored(key).await, (None, None));
    assert_eq!(shop.seen("fail-embedding written by the desk").len(), 1);
}
