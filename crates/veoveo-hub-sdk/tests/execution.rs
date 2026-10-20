use std::collections::BTreeMap;

use veoveo_hub_sdk::{
    CompositionRevision, DeploymentId, EnvironmentId, EnvironmentSelector, ExecutionContext,
    ResolvedComponent, ResolvedComposition, ServerSlug,
};

const ENVIRONMENT: &str = "019a05cb-6c00-7000-8712-3456789abcdf";
const DEPLOYMENT: &str = "019a05cb-6c00-7000-8712-3456789abcde";

fn selected(entries: &[(&str, ResolvedComponent)]) -> ResolvedComposition {
    ResolvedComposition::new(
        EnvironmentId::parse(ENVIRONMENT).unwrap(),
        entries
            .iter()
            .map(|(slug, target)| (ServerSlug::parse(slug).unwrap(), target.clone()))
            .collect(),
    )
}

fn deployment() -> ResolvedComponent {
    ResolvedComponent::Deployment {
        deployment: DeploymentId::parse(DEPLOYMENT).unwrap(),
    }
}

#[test]
fn composition_has_fixed_canonical_bytes_and_order_independent_revision() {
    let first = selected(&[
        ("zeta", ResolvedComponent::Unavailable {}),
        ("alpha", deployment()),
    ]);
    let second = selected(&[
        ("alpha", deployment()),
        ("zeta", ResolvedComponent::Unavailable {}),
    ]);
    assert_eq!(first, second);
    // Fixed vector independently calculated over the exact format URI + NUL + JSON bytes.
    assert_eq!(
        first.revision().digest().as_str(),
        "sha256:61278aa4735ba59c820889390e55edba4d9bbb922548b25cc59059efbfedc1a5"
    );
    assert_eq!(
        String::from_utf8(first.canonical_bytes()).unwrap(),
        format!(
            "{{\"format\":\"https://veoveo.ai/formats/resolved-composition\",\"formatVersion\":1,\"environment\":\"{ENVIRONMENT}\",\"components\":{{\"alpha\":{{\"kind\":\"deployment\",\"deployment\":\"{DEPLOYMENT}\"}},\"zeta\":{{\"kind\":\"unavailable\"}}}}}}"
        )
    );
    assert_eq!(
        ResolvedComposition::from_canonical_bytes(&first.canonical_bytes()).unwrap(),
        first
    );
    let context = first.context();
    assert_eq!(
        context.environment,
        EnvironmentId::parse(ENVIRONMENT).unwrap()
    );
    assert_eq!(&context.composition, first.revision());
    assert_eq!(
        serde_json::from_value::<ExecutionContext>(serde_json::to_value(&context).unwrap())
            .unwrap(),
        context
    );
}

#[test]
fn explicit_unavailability_and_target_changes_cannot_reuse_a_composition() {
    let absent = selected(&[]);
    let unavailable = selected(&[("alpha", ResolvedComponent::Unavailable {})]);
    let serving = selected(&[("alpha", deployment())]);
    let replaced = selected(&[(
        "alpha",
        ResolvedComponent::Deployment {
            deployment: DeploymentId::new(),
        },
    )]);
    assert_ne!(absent.revision(), unavailable.revision());
    assert_ne!(unavailable.revision(), serving.revision());
    assert_ne!(serving.revision(), replaced.revision());
    let other_environment =
        ResolvedComposition::new(EnvironmentId::new(), serving.components().clone());
    assert_ne!(serving.revision(), other_environment.revision());
    // No health or display-name input exists: a stopped selected target stays selected.
    assert_eq!(
        serving.components()[&ServerSlug::parse("alpha").unwrap()],
        deployment()
    );
}

#[test]
fn composition_import_rejects_ambiguous_and_unsupported_records() {
    let composition = selected(&[("alpha", deployment())]);
    let canonical = String::from_utf8(composition.canonical_bytes()).unwrap();
    for invalid in [
        format!(" {canonical}"),
        canonical.replace("\"formatVersion\":1", "\"formatVersion\":2"),
        canonical.replace(
            "\"components\":{",
            "\"components\":{\"alpha\":{\"kind\":\"unavailable\"},",
        ),
        canonical.replace("\"components\":", "\"unknown\":true,\"components\":"),
        canonical.replace(DEPLOYMENT, "not-a-deployment"),
    ] {
        assert!(ResolvedComposition::from_canonical_bytes(invalid.as_bytes()).is_err());
    }
    assert!(
        serde_json::from_value::<ResolvedComponent>(
            serde_json::json!({"kind":"unavailable", "deployment": DEPLOYMENT})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<CompositionRevision>(serde_json::json!("a".repeat(64))).is_err()
    );
    assert!(
        serde_json::from_value::<CompositionRevision>(serde_json::json!(format!(
            "sha256:{}",
            "A".repeat(64)
        )))
        .is_err()
    );
}

#[test]
fn http_selector_is_canonical_identity_only_and_not_an_execution_context() {
    let selection = EnvironmentSelector::parse(ENVIRONMENT).unwrap();
    assert_eq!(
        selection.environment(),
        EnvironmentId::parse(ENVIRONMENT).unwrap()
    );
    for invalid in [
        format!(" {ENVIRONMENT}"),
        ENVIRONMENT.to_uppercase(),
        format!("{ENVIRONMENT},{ENVIRONMENT}"),
        "base".to_owned(),
    ] {
        assert!(EnvironmentSelector::parse(&invalid).is_err());
    }
    assert!(
        serde_json::from_value::<ExecutionContext>(serde_json::json!({"environment": ENVIRONMENT}))
            .is_err()
    );
    assert!(
        ResolvedComposition::new(selection.environment(), BTreeMap::new())
            .components()
            .is_empty()
    );
}
