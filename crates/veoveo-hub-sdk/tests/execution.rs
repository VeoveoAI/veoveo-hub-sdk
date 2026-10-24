use veoveo_hub_sdk::{
    CompositionRevision, DeploymentId, ExecutionContext, ResolvedComponent, ResolvedComposition,
    ServerSlug,
};

const DEPLOYMENT: &str = "019a05cb-6c00-7000-8712-3456789abcde";

fn selected(entries: &[(&str, ResolvedComponent)]) -> ResolvedComposition {
    ResolvedComposition::new(
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
        "sha256:e1cfdb6b62341411df4e646973a6159286ce60dde568b355a409659afabd5a53"
    );
    assert_eq!(
        String::from_utf8(first.canonical_bytes()).unwrap(),
        format!(
            "{{\"format\":\"https://veoveo.ai/formats/resolved-composition\",\"formatVersion\":2,\"components\":{{\"alpha\":{{\"kind\":\"deployment\",\"deployment\":\"{DEPLOYMENT}\"}},\"zeta\":{{\"kind\":\"unavailable\"}}}}}}"
        )
    );
    assert_eq!(
        ResolvedComposition::from_canonical_bytes(&first.canonical_bytes()).unwrap(),
        first
    );
    let context = first.context();
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
        // Version 1 named an environment; it is no longer read.
        canonical.replace("\"formatVersion\":2", "\"formatVersion\":1"),
        canonical.replace(
            "\"components\":",
            "\"environment\":\"019a05cb-6c00-7000-8712-3456789abcdf\",\"components\":",
        ),
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
fn an_execution_context_carries_only_its_composition() {
    let context = selected(&[("alpha", deployment())]).context();
    assert_eq!(
        serde_json::to_value(&context).unwrap(),
        serde_json::json!({"composition": context.composition})
    );
    assert!(
        serde_json::from_value::<ExecutionContext>(serde_json::json!({
            "environment": "019a05cb-6c00-7000-8712-3456789abcdf",
            "composition": context.composition,
        }))
        .is_err(),
        "a context that names an environment is refused"
    );
}
