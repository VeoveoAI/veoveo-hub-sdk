//! The `contract` feature carries declarations and wire types only. Consumers that
//! select it must not compile the server runtime.
use std::process::Command;

const RUNTIME_PACKAGES: [&str; 5] = ["rmcp", "tokio", "axum", "reqwest", "sqlx"];

#[test]
fn the_contract_feature_pulls_in_no_server_runtime() {
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
            "contract",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
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
    let packages: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(packages.contains(&"veoveo-sdk-rust"), "{tree}");
    for runtime in RUNTIME_PACKAGES {
        assert!(
            !packages.contains(&runtime),
            "the contract feature depends on {runtime}:\n{tree}"
        );
    }
}
