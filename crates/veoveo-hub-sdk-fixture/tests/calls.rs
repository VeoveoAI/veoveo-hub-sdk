//! Calls between servers through the SDK's in-process mesh: Relay calls Ledger for its
//! own caller inline, through a Ledger Task, and from a Ledger Task back to Relay,
//! reads and follows a Ledger entry through typed resource reads, and calls a Hub route
//! for its caller.
use std::time::Duration;

use axum::{Extension, Json, extract::Path};
use serde_json::{Value, json};
use veoveo_hub_sdk::{
    ExchangedIdentity,
    calls::TokenErrorCode,
    client::CallError,
    hosting::testing::{self, MESH_PROFILE, TEST_CLIENT, TestMesh},
    registration::AuthorizationDeclaration,
};
use veoveo_hub_sdk_fixture::{
    calls::{EntryState, FollowedEntry, HubCaller, HubCalls, Inspection},
    gateway::{self, Gateway},
    ledger::{self, LedgerDomain, LedgerTasks},
    relay::{self, RelayDomain},
};
use veoveo_types::{PermissionName, TaskId};

fn permission(name: &str) -> PermissionName {
    PermissionName::parse(name).unwrap()
}

/// What a relayed call answered: Relay's structured result, or its error text.
fn relayed(answer: &Value) -> Result<Value, String> {
    let result = &answer["result"];
    if result["isError"] == json!(true) {
        return Err(result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned());
    }
    result
        .get("structuredContent")
        .cloned()
        .ok_or_else(|| format!("no result: {answer}"))
}

/// A loopback mesh. reqwest is built without a TLS provider, as in every component.
async fn bind() -> TestMesh {
    let _ = rustls::crypto::ring::default_provider().install_default();
    TestMesh::bind().await
}

/// Relay and Ledger on one mesh; the user holds `granted`.
async fn mesh(granted: &[&str], with_ledger: bool) -> testing::RunningMesh {
    let mut mesh = bind().await;
    let calls = mesh.register(relay::authorization().unwrap());
    mesh.serve(
        testing::for_domain::<RelayDomain>()
            .calls(calls)
            .handler(relay::hosted)
            .build(),
    );
    if with_ledger {
        mesh.register(ledger::authorization().unwrap());
        let tasks = LedgerTasks::default();
        mesh.serve(
            testing::for_domain::<LedgerDomain>()
                .handler(move || ledger::hosted(tasks.clone()))
                .build(),
        );
    }
    mesh.granting(granted.iter().map(|name| permission(name)))
        .start()
}

#[tokio::test]
async fn relay_calls_ledger_for_its_caller_with_the_callers_permissions() {
    let mesh = mesh(&["relay.calls.make", "ledger.entries.read"], true).await;
    let inspection: Inspection = serde_json::from_value(
        relayed(&mesh.call("relay", "relay_inspect", json!({})).await).unwrap(),
    )
    .unwrap();
    assert_eq!(inspection.caller, testing::principal().id.to_string());
    assert_eq!(inspection.client, TEST_CLIENT);
    assert_eq!(inspection.acting_server.as_deref(), Some("relay"));
    assert_eq!(inspection.permissions, ["ledger.entries.read"]);

    // Relay declares ledger.seals.read, but this caller does not hold it.
    let refused = relayed(&mesh.call("relay", "relay_seal", json!({})).await).unwrap_err();
    assert!(refused.contains("ledger.seals.read"), "{refused}");
}

#[tokio::test]
async fn a_caller_holding_the_permission_reads_through_relay() {
    let mesh = mesh(
        &[
            "relay.calls.make",
            "ledger.entries.read",
            "ledger.seals.read",
        ],
        true,
    )
    .await;
    let seal = relayed(&mesh.call("relay", "relay_seal", json!({})).await).unwrap();
    assert_eq!(seal["value"], "seal-7");
}

async fn three_servers(
    granted: &[&str],
    declared: bool,
    relay_declared: bool,
) -> testing::RunningMesh {
    let mut mesh = bind().await;
    let mut declaration = gateway::authorization();
    if !declared {
        declaration.uses.clear();
    }
    let gateway_calls = mesh.register(declaration);
    mesh.serve(
        testing::for_domain::<Gateway>()
            .calls(gateway_calls)
            .handler(gateway::hosted)
            .build(),
    );
    let mut relay_declaration = relay::authorization().unwrap();
    if !relay_declared {
        relay_declaration.uses.clear();
    }
    let relay_calls = mesh.register(relay_declaration);
    mesh.serve(
        testing::for_domain::<RelayDomain>()
            .calls(relay_calls)
            .handler(relay::hosted)
            .build(),
    );
    mesh.register(ledger::authorization().unwrap());
    mesh.serve(
        testing::for_domain::<LedgerDomain>()
            .handler(|| ledger::hosted(LedgerTasks::default()))
            .build(),
    );
    mesh.granting(
        granted
            .iter()
            .map(|name| permission(name))
            .chain([permission("gateway.calls.make")]),
    )
    .start()
}

#[tokio::test]
async fn three_servers_preserve_the_caller_and_both_server_actors() {
    let mesh = three_servers(&["relay.calls.make", "ledger.entries.read"], true, true).await;
    let seen: Inspection = serde_json::from_value(
        relayed(&mesh.call("gateway", "gateway_inspect", json!({})).await).unwrap(),
    )
    .unwrap();
    assert_eq!(seen.caller, testing::principal().id.to_string());
    assert_eq!(seen.client, TEST_CLIENT);
    assert_eq!(seen.acting_server.as_deref(), Some("relay"));
    assert_eq!(seen.prior_servers, ["gateway"]);
    assert_eq!(seen.originating_task, None);
    assert_eq!(seen.permissions, ["ledger.entries.read"]);
}

#[tokio::test]
async fn nested_calls_require_each_servers_uses_and_the_callers_permission() {
    let missing_use =
        three_servers(&["relay.calls.make", "ledger.entries.read"], false, true).await;
    let refused = relayed(
        &missing_use
            .call("gateway", "gateway_inspect", json!({}))
            .await,
    )
    .unwrap_err();
    assert!(refused.contains("declares no use of relay"), "{refused}");
    let missing_nested_use =
        three_servers(&["relay.calls.make", "ledger.entries.read"], true, false).await;
    let refused = relayed(
        &missing_nested_use
            .call("gateway", "gateway_inspect", json!({}))
            .await,
    )
    .unwrap_err();
    assert!(refused.contains("declares no use of ledger"), "{refused}");
    let missing_permission = three_servers(&["relay.calls.make"], true, true).await;
    let refused = relayed(
        &missing_permission
            .call("gateway", "gateway_inspect", json!({}))
            .await,
    )
    .unwrap_err();
    // Native discovery hides a provider whose permissions this caller does not hold.
    assert!(refused.contains("404 Not Found"), "{refused}");
}

/// A Hub route that reports the exchanged token the mesh admitted for it.
async fn hub_caller(
    Path(profile): Path<String>,
    Extension(identity): Extension<ExchangedIdentity>,
) -> Json<HubCaller> {
    assert_eq!(profile, MESH_PROFILE);
    Json(HubCaller {
        caller: identity.caller.actor.id.to_string(),
        acting_server: identity.acting_server.server.to_string(),
        permissions: identity
            .permissions
            .iter()
            .map(ToString::to_string)
            .collect(),
        token: identity.jwt_id.to_string(),
    })
}

/// Relay as `relay` declares, Ledger, and the Hub route `/hub/{profile}/caller`.
async fn hub_mesh(relay: AuthorizationDeclaration, granted: &[&str]) -> testing::RunningMesh {
    let mut mesh = bind().await;
    let calls = mesh.register(relay);
    mesh.serve(
        testing::for_domain::<RelayDomain>()
            .calls(calls)
            .handler(relay::hosted)
            .build(),
    );
    mesh.register(ledger::authorization().unwrap());
    mesh.serve(
        testing::for_domain::<LedgerDomain>()
            .handler(|| ledger::hosted(LedgerTasks::default()))
            .build(),
    );
    mesh.hub_route(
        axum::Router::new().route("/hub/{profile}/caller", axum::routing::get(hub_caller)),
    );
    mesh.granting(granted.iter().map(|name| permission(name)))
        .start()
}

#[tokio::test]
async fn relay_calls_a_hub_route_for_its_caller_with_a_token_naming_no_permissions() {
    let mesh = hub_mesh(
        relay::authorization().unwrap(),
        &["relay.calls.make", "ledger.entries.read"],
    )
    .await;
    let calls: HubCalls =
        serde_json::from_value(relayed(&mesh.call("relay", "relay_hub", json!({})).await).unwrap())
            .unwrap();
    assert_eq!(calls.first.caller, testing::principal().id.to_string());
    assert_eq!(calls.first.acting_server, "relay");
    assert!(calls.first.permissions.is_empty(), "{calls:?}");
    // The Hub token is cached for the request apart from Ledger's, which carries
    // Relay's use of Ledger.
    assert_eq!(calls.again, calls.first);
    assert_eq!(calls.inspection.permissions, ["ledger.entries.read"]);

    // A Hub route admits only a token the mesh exchanged.
    let mut route = mesh.base();
    route.set_path(&format!("/hub/{MESH_PROFILE}/caller"));
    let anonymous = reqwest::get(route.clone()).await.unwrap();
    assert_eq!(anonymous.status(), reqwest::StatusCode::UNAUTHORIZED);
    let forged = reqwest::Client::new()
        .get(route)
        .bearer_auth("not-a-token")
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_server_that_neither_uses_others_nor_calls_the_hub_exchanges_no_token() {
    let mut declaration = relay::authorization().unwrap();
    declaration.uses.clear();
    declaration.calls_hub = false;
    let mesh = hub_mesh(declaration, &["relay.calls.make"]).await;
    let refused = relayed(&mesh.call("relay", "relay_hub_caller", json!({})).await).unwrap_err();
    assert!(refused.contains("UnauthorizedClient"), "{refused}");

    // Calling the Hub alone holds the grant.
    let mut declaration = relay::authorization().unwrap();
    declaration.uses.clear();
    let mesh = hub_mesh(declaration, &["relay.calls.make"]).await;
    let seen: HubCaller = serde_json::from_value(
        relayed(&mesh.call("relay", "relay_hub_caller", json!({})).await).unwrap(),
    )
    .unwrap();
    assert_eq!(seen.caller, testing::principal().id.to_string());
    assert_eq!(seen.acting_server, "relay");
    assert!(seen.permissions.is_empty(), "{seen:?}");
}

#[tokio::test]
async fn a_task_handle_resolves_with_the_providers_task_result() {
    let mesh = mesh(&["relay.calls.make", "ledger.entries.read"], true).await;
    let tallied = relayed(
        &mesh
            .call("relay", "relay_tally", json!({"entries": 3}))
            .await,
    )
    .unwrap();
    assert_eq!(tallied["entries"], 3);
    assert_eq!(tallied["inspection"]["acting_server"], "relay");
}

#[tokio::test]
async fn a_missing_provider_is_named_in_the_refusal() {
    let mesh = mesh(&["relay.calls.make", "ledger.entries.read"], false).await;
    let refused = relayed(&mesh.call("relay", "relay_inspect", json!({})).await).unwrap_err();
    assert!(refused.contains("InvalidTarget"), "{refused}");
    assert!(refused.contains("ledger.entries.read"), "{refused}");
}

#[tokio::test]
async fn an_undeclared_provider_is_refused_before_any_exchange() {
    let mut mesh = bind().await;
    let mut declaration = relay::authorization().unwrap();
    declaration.uses.clear();
    let calls = mesh.register(declaration);
    mesh.serve(
        testing::for_domain::<RelayDomain>()
            .calls(calls)
            .handler(relay::hosted)
            .build(),
    );
    mesh.register(ledger::authorization().unwrap());
    let mesh = mesh
        .granting([
            permission("relay.calls.make"),
            permission("ledger.entries.read"),
        ])
        .start();
    let refused = relayed(&mesh.call("relay", "relay_inspect", json!({})).await).unwrap_err();
    assert!(refused.contains("declares no use of ledger"), "{refused}");
}

/// A Ledger Task calls Relay for the Task's caller while it runs. Relay's own call
/// back to Ledger preserves the original Task constraint through Relay, and the
/// Task's delegation ends with the Task.
#[tokio::test]
async fn a_task_calls_for_its_caller_while_it_runs() {
    let mut mesh = bind().await;
    let relay_calls = mesh.register(relay::authorization().unwrap());
    mesh.serve(
        testing::for_domain::<RelayDomain>()
            .calls(relay_calls)
            .handler(relay::hosted)
            .build(),
    );
    let mut declaration = ledger::authorization().unwrap();
    declaration.uses.push(permission("relay.calls.make"));
    let ledger_calls = mesh.register(declaration);
    let tasks = LedgerTasks::new(Duration::from_secs(2));
    mesh.serve(
        testing::for_domain::<LedgerDomain>()
            .handler(move || ledger::hosted(tasks.clone()))
            .build(),
    );
    let mesh = mesh
        .granting([
            permission("relay.calls.make"),
            permission("ledger.entries.read"),
        ])
        .start();
    let started = mesh
        .start_task("ledger", "tally", json!({"entries": 1}))
        .await;
    let task = started["result"]["taskId"]
        .as_str()
        .unwrap_or_else(|| panic!("no Task started: {started}"))
        .to_owned();
    let task_id = TaskId::parse(task.clone()).unwrap();
    let profile = veoveo_hub_sdk::HubProfileId::parse(MESH_PROFILE).unwrap();

    // Relay sees Ledger as the acting server and the Task it called from.
    let from_task = ledger_calls.for_task_id(task_id, &profile);
    let seen: Inspection = from_task.call(RelayWhoami {}).await.unwrap();
    assert_eq!(seen.caller, testing::principal().id.to_string());
    assert_eq!(seen.acting_server.as_deref(), Some("ledger"));
    assert_eq!(seen.acting_task.as_deref(), Some(task.as_str()));
    assert_eq!(seen.permissions, ["relay.calls.make"]);

    // Relay's nested request names Relay immediately and retains Ledger's Task.
    let relayed_back: Inspection = from_task.call(RelayInspect {}).await.unwrap();
    assert_eq!(relayed_back.caller, testing::principal().id.to_string());
    assert_eq!(relayed_back.client, TEST_CLIENT);
    assert_eq!(relayed_back.acting_server.as_deref(), Some("relay"));
    assert_eq!(relayed_back.acting_task, None);
    assert_eq!(relayed_back.prior_servers, ["ledger"]);
    assert_eq!(
        relayed_back.originating_task,
        Some(("ledger".to_owned(), task.clone()))
    );

    // Once the Task ends, it no longer acts for its caller.
    tokio::time::timeout(Duration::from_secs(10), async {
        while mesh.task("ledger", &task).await["result"]["status"] != "completed" {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let ended = ledger_calls
        .for_task_id(task_id, &profile)
        .call(RelayInspect {})
        .await;
    assert!(
        matches!(
            ended,
            Err(CallError::Refused {
                error: TokenErrorCode::InvalidGrant,
                ..
            })
        ),
        "{ended:?}"
    );
}

/// Relay opens a Ledger entry and follows it: the typed updates show it open, then
/// closed, a wait for the closed state resolves at once, and a plain read agrees.
#[tokio::test]
async fn relay_follows_a_ledger_entry_until_it_closes() {
    let mesh = mesh(
        &[
            "relay.calls.make",
            "ledger.entries.read",
            "ledger.entries.write",
        ],
        true,
    )
    .await;
    let followed: FollowedEntry = serde_json::from_value(
        relayed(
            &mesh
                .call("relay", "relay_follow_entry", json!({"entry": "batch-1"}))
                .await,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(followed.states, [EntryState::Open, EntryState::Closed]);
    assert_eq!(followed.waited.state, EntryState::Closed);
    assert_eq!(followed.waited.entry.as_str(), "batch-1");
    assert_eq!(followed.read, followed.waited);
}

/// A read carries only the permissions the caller holds: without the read permission
/// Ledger refuses both the subscription and the read.
#[tokio::test]
async fn a_read_needs_the_callers_read_permission() {
    let mesh = mesh(&["relay.calls.make", "ledger.entries.write"], true).await;
    let refused = relayed(
        &mesh
            .call("relay", "relay_follow_entry", json!({"entry": "batch-2"}))
            .await,
    )
    .unwrap_err();
    assert!(refused.contains("ledger.entries.read"), "{refused}");
}

/// Relay's `relay_inspect` as a typed call, for a server that uses Relay.
#[derive(serde::Serialize)]
struct RelayInspect {}

/// Relay's `relay_whoami`: the identity a call reached Relay with.
#[derive(serde::Serialize)]
struct RelayWhoami {}

impl veoveo_hub_sdk::calls::ToolCall for RelayWhoami {
    type Provider = Relay;
    type Output = Inspection;
    const TOOL: &'static str = "relay_whoami";
}

struct Relay;
impl veoveo_hub_sdk::calls::ProviderContract for Relay {
    fn slug() -> veoveo_types::ServerSlug {
        veoveo_types::ServerSlug::parse("relay").unwrap()
    }
}
impl veoveo_hub_sdk::calls::ToolCall for RelayInspect {
    type Provider = Relay;
    type Output = Inspection;
    const TOOL: &'static str = "relay_inspect";
}
