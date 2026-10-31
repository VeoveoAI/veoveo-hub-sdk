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

#[test]
fn a_linked_component_names_one_runner_per_link() {
    let other = "019a05cb-6c00-7000-8712-3456789abcdf";
    let link = |name: &str| veoveo_hub_sdk::hub::LinkName::parse(name).unwrap();
    let runners = ResolvedComponent::Runners {
        runners: [
            (link("team-b"), DeploymentId::parse(other).unwrap()),
            (link("team-a"), DeploymentId::parse(DEPLOYMENT).unwrap()),
        ]
        .into_iter()
        .collect(),
    };
    let composition = selected(&[("n8n", runners.clone())]);
    // Fixed vector independently calculated over the exact format URI + NUL + JSON bytes.
    assert_eq!(
        String::from_utf8(composition.canonical_bytes()).unwrap(),
        format!(
            "{{\"format\":\"https://veoveo.ai/formats/resolved-composition\",\"formatVersion\":2,\"components\":{{\"n8n\":{{\"kind\":\"runners\",\"runners\":{{\"team-a\":\"{DEPLOYMENT}\",\"team-b\":\"{other}\"}}}}}}}}"
        )
    );
    assert_eq!(
        composition.revision().digest().as_str(),
        "sha256:b641b957c662fb7152fea2297e390291af6c26b92818cc70ecfeea099c0c24e7"
    );
    assert_eq!(
        ResolvedComposition::from_canonical_bytes(&composition.canonical_bytes()).unwrap(),
        composition
    );
    // Each link routes to its own runner, and neither no link nor an unknown one routes.
    assert_eq!(
        runners.deployment(Some(&link("team-b"))),
        Some(DeploymentId::parse(other).unwrap())
    );
    assert_eq!(runners.deployment(None), None);
    assert_eq!(runners.deployment(Some(&link("team-c"))), None);
    assert_eq!(deployment().deployment(Some(&link("team-a"))), None);
    assert_eq!(runners.deployments().len(), 2);
    let canonical = String::from_utf8(composition.canonical_bytes()).unwrap();
    for invalid in [
        canonical.replace("team-a", "Team_A"),
        canonical.replace(
            "\"runners\":{",
            "\"runners\":{\"\":\"019a05cb-6c00-7000-8712-3456789abcd0\",",
        ),
    ] {
        assert!(ResolvedComposition::from_canonical_bytes(invalid.as_bytes()).is_err());
    }
}
