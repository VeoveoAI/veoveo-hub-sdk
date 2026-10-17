use veoveo_hub_sdk::{hub::HubToolName, identity::LocalToolName};
use veoveo_types::{Identity, ServerSlug};

#[test]
fn hub_tool_names_project_the_server_namespace() {
    let tool = HubToolName::from_parts(
        &ServerSlug::parse("media").unwrap(),
        &LocalToolName::parse("render").unwrap(),
    )
    .unwrap();
    assert_eq!(tool.as_str(), "media__render");
    let (server, local) = tool.parts().unwrap();
    assert_eq!(server.as_str(), "media");
    assert_eq!(local.identity_text(), "render");

    let schema = serde_json::to_value(schemars::schema_for!(LocalToolName)).unwrap();
    assert_eq!(schema["type"], "string");
    assert!(LocalToolName::parse("bad/name").is_err());
}
