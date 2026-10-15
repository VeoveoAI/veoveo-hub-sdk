//! Each feature compiles only what it uses. The `contract` feature carries declarations
//! and wire types, so its consumers compile no MCP stack or runtime. The `client`
//! feature calls servers through a Gateway, so it compiles rmcp's client and its
//! Streamable HTTP transport, and none of rmcp's server, its macros or the server's
//! HTTP, token and database stacks. Each test resolves the crate with default features
//! off and one feature on, over its normal and build dependencies, as a consumer's
//! build resolves it; the crate's own dev-dependencies do not join in.
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
};

const RUNTIME_PACKAGES: [&str; 5] = ["rmcp", "tokio", "axum", "reqwest", "sqlx"];

/// Packages a client never compiles: rmcp's macros and the server's HTTP, token,
/// telemetry and database stacks.
const SERVER_PACKAGES: [&str; 9] = [
    "rmcp-macros",
    "axum",
    "jsonwebtoken",
    "opentelemetry",
    "opentelemetry_sdk",
    "tracing-opentelemetry",
    "sqlx",
    "veoveo-task",
    "veoveo-store",
];

/// rmcp features that belong to a server.
const SERVER_RMCP_FEATURES: [&str; 6] = [
    "server",
    "macros",
    "schemars",
    "server-side-http",
    "transport-streamable-http-server",
    "transport-streamable-http-server-session",
];

/// Each package the crate reaches with only `feature` on, with the features it
/// compiles with.
fn reached(feature: &str) -> BTreeMap<String, BTreeSet<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--offline",
            "--package",
            "veoveo-sdk-rust",
            "--no-default-features",
            "--features",
            feature,
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    let mut packages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in tree.lines() {
        let Some((package, features)) = line.split_once('|') else {
            continue;
        };
        let Some(name) = package.split_whitespace().next() else {
            continue;
        };
        let features = features.trim().trim_end_matches("(*)").trim();
        packages.entry(name.to_owned()).or_default().extend(
            features
                .split(',')
                .filter(|feature| !feature.is_empty())
                .map(str::to_owned),
        );
    }
    packages
}

#[test]
fn the_contract_feature_pulls_in_no_server_runtime() {
    let packages = reached("contract");
    assert!(packages.contains_key("veoveo-sdk-rust"), "{packages:?}");
    for runtime in RUNTIME_PACKAGES {
        assert!(
            !packages.contains_key(runtime),
            "the contract feature depends on {runtime}: {packages:?}"
        );
    }
}

#[test]
fn the_client_feature_compiles_rmcp_as_a_client_only() {
    let packages = reached("client");
    let rmcp = packages
        .get("rmcp")
        .unwrap_or_else(|| panic!("the client feature reaches rmcp: {packages:?}"));
    for expected in ["client", "transport-streamable-http-client-reqwest"] {
        assert!(rmcp.contains(expected), "rmcp lacks {expected}: {rmcp:?}");
    }
    let server_features: Vec<&&str> = SERVER_RMCP_FEATURES
        .iter()
        .filter(|feature| rmcp.contains(**feature))
        .collect();
    assert!(
        server_features.is_empty(),
        "the client feature compiles rmcp's {server_features:?}"
    );
    let server_packages: Vec<&&str> = SERVER_PACKAGES
        .iter()
        .filter(|package| packages.contains_key(**package))
        .collect();
    assert!(
        server_packages.is_empty(),
        "the client feature reaches {server_packages:?}"
    );
}

#[test]
fn the_server_feature_still_compiles_the_hosting_stack() {
    let packages = reached("server");
    let rmcp = &packages["rmcp"];
    for expected in ["server", "transport-streamable-http-server", "client"] {
        assert!(rmcp.contains(expected), "rmcp lacks {expected}: {rmcp:?}");
    }
    assert!(
        !rmcp.contains("macros"),
        "the SDK itself uses no rmcp macros"
    );
    assert!(!packages.contains_key("rmcp-macros"), "{packages:?}");
}
