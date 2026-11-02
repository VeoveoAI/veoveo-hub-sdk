//! Observatory's skills through the Hub's forwarding, as a host reads them: `skills/list`
//! and `skills/get` answer each skill's frontmatter and manifest, and `resources/read`
//! answers each file with the bytes the manifest names.
use serde_json::{Value, json};
use veoveo_hub_sdk::hosting::{
    DomainServer,
    testing::{self, TestMesh},
};
use veoveo_hub_sdk_fixture::mcp::{self, ObservatoryMcp};
use veoveo_types::ServerSlug;

async fn mesh() -> testing::RunningMesh {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut mesh = TestMesh::bind().await;
    mesh.register_server(
        ServerSlug::parse("observatory").unwrap(),
        ObservatoryMcp::routes().authorization(),
    );
    mesh.serve(
        testing::for_domain::<ObservatoryMcp>()
            .handler(mcp::hosted)
            .build(),
    );
    mesh.start()
}

#[tokio::test]
async fn a_hosted_server_serves_its_embedded_skills() {
    let mesh = mesh().await;
    let listed = mesh.request("observatory", "skills/list", json!({})).await;
    let skills = listed["result"]["skills"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(skills.len(), 1);
    let skill = &skills[0];
    assert_eq!(skill["uri"], "skill://observatory/read-readings/SKILL.md");
    assert_eq!(skill["frontmatter"]["name"], "read-readings");
    assert_eq!(skill["frontmatter"]["metadata"]["audience"], "test");
    let files: Vec<&str> = skill["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            "skill://observatory/read-readings/SKILL.md",
            "skill://observatory/read-readings/references/units.md"
        ]
    );
    let got = mesh
        .request(
            "observatory",
            "skills/get",
            json!({"uri": "skill://observatory/read-readings/SKILL.md"}),
        )
        .await;
    assert_eq!(got["result"]["skill"], *skill, "{got}");
    // Each file reads with the size the manifest names.
    for file in skill["resources"].as_array().unwrap() {
        let read = mesh
            .request("observatory", "resources/read", json!({"uri": file["uri"]}))
            .await;
        let text = read["result"]["contents"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{read}"));
        assert_eq!(text.len() as u64, file["size"].as_u64().unwrap());
        assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");
    }
    let missing: Value = mesh
        .request(
            "observatory",
            "resources/read",
            json!({"uri": "skill://observatory/read-readings/missing.md"}),
        )
        .await;
    assert!(missing["error"].is_object(), "{missing}");
    let unknown = mesh
        .request(
            "observatory",
            "skills/get",
            json!({"uri": "skill://observatory/unknown/SKILL.md"}),
        )
        .await;
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
}
