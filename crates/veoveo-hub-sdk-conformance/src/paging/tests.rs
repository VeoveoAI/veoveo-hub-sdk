use super::*;
use std::{collections::VecDeque, future::ready};

fn limits() -> PagingLimits {
    PagingLimits {
        page_items: 2,
        response_bytes: 1024,
        cursor_bytes: 32,
    }
}

fn page(keys: &[u32], cursor: Option<&str>) -> ObservedPage<u32> {
    ObservedPage::from_response(
        &json!({"items": keys, "nextCursor": cursor}),
        keys.to_vec(),
        cursor.map(str::to_owned),
    )
    .unwrap()
}

async fn inspect(expected: &[u32], pages: Vec<ObservedPage<u32>>) -> PagingObservation {
    let mut pages = VecDeque::from(pages);
    inspect_pages(
        ServerSlug::parse("fixture").unwrap(),
        NamingLabel::new("fixture-resource-items").unwrap(),
        PagingSurface::Resource,
        PagingOrigin::SyntheticFixture,
        &limits(),
        expected,
        |_| {
            ready(
                pages
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("secret fetch error")),
            )
        },
    )
    .await
}

fn assess(observations: &[PagingObservation]) -> CheckResult {
    check_paging(
        &PagingEvidence {
            required_listings: &[NamingLabel::new("fixture-resource-items").unwrap()],
            observations,
        },
        "fixture",
    )
}

#[tokio::test]
async fn traversal_checks_complete_ordered_selection_and_boundary_cases() {
    let multi = inspect(
        &[1, 2, 3],
        vec![page(&[1, 2], Some("next")), page(&[3], None)],
    )
    .await;
    let empty = inspect(&[], vec![page(&[], None)]).await;
    let exact = inspect(&[1, 2], vec![page(&[1, 2], None)]).await;
    let final_empty = inspect(&[1, 2], vec![page(&[1, 2], Some("next")), page(&[], None)]).await;
    assert_eq!(
        assess(&[multi, empty, exact, final_empty]).status,
        CheckStatus::Passed
    );
}

#[tokio::test]
async fn empty_token_is_followed_verbatim() {
    let mut calls = 0;
    let observation = inspect_pages(
        ServerSlug::parse("fixture").unwrap(),
        NamingLabel::new("fixture-resource-items").unwrap(),
        PagingSurface::Resource,
        PagingOrigin::SyntheticFixture,
        &limits(),
        &[1, 2, 3],
        |cursor| {
            calls += 1;
            if calls == 1 {
                assert_eq!(cursor, None);
                ready(Ok(page(&[1, 2], Some(""))))
            } else {
                assert_eq!(cursor.as_deref(), Some(""));
                ready(Ok(page(&[3], None)))
            }
        },
    )
    .await;
    assert_eq!(observation.status(), CheckStatus::Passed);
    assert_eq!(calls, 2);
}

#[tokio::test]
async fn rejects_repeated_cursor_duplicates_missing_and_reordering() {
    for (expected, pages) in [
        (
            vec![1, 2, 3],
            vec![page(&[1], Some("same")), page(&[2], Some("same"))],
        ),
        (
            vec![1, 2, 3],
            vec![page(&[1, 2], Some("next")), page(&[2, 3], None)],
        ),
        (vec![1, 2, 3], vec![page(&[1, 2], None)]),
        (vec![1, 2, 3], vec![page(&[2, 1], Some("next"))]),
        (vec![1], vec![page(&[1, 2], None)]),
    ] {
        let observation = inspect(&expected, pages).await;
        assert_eq!(observation.status(), CheckStatus::Failed);
        assert_eq!(assess(&[observation]).status, CheckStatus::Failed);
    }
}

#[tokio::test]
async fn empty_intermediate_page_is_not_completion() {
    let observed = inspect(
        &[1, 2],
        vec![
            page(&[], Some("first")),
            page(&[1], Some("second")),
            page(&[2], None),
        ],
    )
    .await;
    assert_eq!(assess(&[observed]).status, CheckStatus::Passed);
}

#[tokio::test]
async fn rejects_page_and_cursor_limits_and_invalid_expected_selection() {
    let observation = inspect(&[1, 2, 3], vec![page(&[1, 2, 3], None)]).await;
    assert_eq!(observation.status(), CheckStatus::Failed);
    let observation = inspect(&[1, 2], vec![page(&[1], Some(&"x".repeat(33)))]).await;
    assert_eq!(observation.status(), CheckStatus::Failed);
    let observation = inspect(&[2, 1], vec![]).await;
    assert_eq!(observation.status(), CheckStatus::Failed);
    let observation = inspect_pages(
        ServerSlug::parse("fixture").unwrap(),
        NamingLabel::new("fixture-resource-items").unwrap(),
        PagingSurface::Resource,
        PagingOrigin::SyntheticFixture,
        &PagingLimits {
            response_bytes: 1,
            ..limits()
        },
        &[1],
        |_| ready(Ok(page(&[1], None))),
    )
    .await;
    assert_eq!(observation.status(), CheckStatus::Failed);
}

#[tokio::test]
async fn missing_single_page_and_other_listings_cannot_supply_continuation() {
    assert_eq!(
        check_paging(&Default::default(), "fixture").status,
        CheckStatus::Incomplete
    );
    let single = inspect(&[1], vec![page(&[1], None)]).await;
    assert_eq!(assess(&[single]).status, CheckStatus::Incomplete);
    let observations = [inspect(
        &[1, 2, 3],
        vec![page(&[1, 2], Some("next")), page(&[3], None)],
    )
    .await];
    let required = [
        NamingLabel::new("fixture-resource-items").unwrap(),
        NamingLabel::new("other").unwrap(),
    ];
    assert_eq!(
        check_paging(
            &PagingEvidence {
                required_listings: &required,
                observations: &observations
            },
            "fixture"
        )
        .status,
        CheckStatus::Incomplete
    );
    assert_eq!(
        check_paging(
            &PagingEvidence {
                required_listings: &required[..1],
                observations: &observations
            },
            "different"
        )
        .status,
        CheckStatus::Failed
    );
    let duplicate = [required[0].clone(), required[0].clone()];
    assert_eq!(
        check_paging(
            &PagingEvidence {
                required_listings: &duplicate,
                observations: &observations
            },
            "fixture"
        )
        .status,
        CheckStatus::Failed
    );
}

#[tokio::test]
async fn transport_errors_stay_incomplete_and_evidence_omits_values_and_cursors() {
    let failed = inspect(&[1], vec![]).await;
    assert_eq!(failed.status(), CheckStatus::Incomplete);
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("secret fetch error")
    );
    let observed = inspect(
        &[91, 92, 93],
        vec![page(&[91, 92], Some("secret-cursor")), page(&[93], None)],
    )
    .await;
    let check = assess(&[observed]);
    let text = serde_json::to_string(&check).unwrap();
    assert!(!text.contains("secret-cursor"));
    assert!(!text.contains("[91"));
    assert_eq!(check.evidence.unwrap()["outcome"], "review_required");
}

#[test]
fn response_serialization_is_bounded_without_a_second_payload_allocation() {
    let oversized = "x".repeat(veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES);
    assert!(ObservedPage::<u32>::from_response(&oversized, vec![], None).is_err());
}

#[tokio::test]
async fn known_oversized_result_fails_and_reports_sanitized_reason() {
    let oversized = "x".repeat(veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES);
    let observation = inspect_pages(
        ServerSlug::parse("fixture").unwrap(),
        NamingLabel::new("fixture-resource-items").unwrap(),
        PagingSurface::Resource,
        PagingOrigin::SyntheticFixture,
        &limits(),
        &[1u32],
        |_| ready(ObservedPage::from_response(&oversized, vec![1], None)),
    )
    .await;
    let result = assess(&[observation]);
    assert_eq!(result.status, CheckStatus::Failed);
    assert_eq!(
        result.evidence.unwrap()["observations"][0]["reason"],
        "serialized MCP result exceeds response byte cap"
    );
}

#[tokio::test]
async fn aggregate_budgets_and_rejected_report_size_are_bounded() {
    let base = inspect(
        &[1, 2, 3],
        vec![page(&[1, 2], Some("next")), page(&[3], None)],
    )
    .await;
    for budget in ["pages", "items", "bytes", "time"] {
        let mut observation = base.clone();
        match budget {
            "pages" => observation.pages = MAX_PAGING_PAGES + 1,
            "items" => observation.items = MAX_PAGING_ITEMS + 1,
            "bytes" => observation.response_bytes = MAX_PAGING_BYTES + 1,
            _ => observation.elapsed_micros = PAGING_DEADLINE.as_micros() + 1,
        }
        assert_eq!(assess(&[observation]).status, CheckStatus::Incomplete);
    }
    let too_many = vec![base; MAX_PAGING_SELECTIONS + 1];
    let result = assess(&too_many);
    assert_eq!(result.status, CheckStatus::Incomplete);
    let evidence = result.evidence.unwrap();
    assert_eq!(evidence["observationCount"], MAX_PAGING_SELECTIONS + 1);
    assert_eq!(
        evidence["observations"].as_array().unwrap().len(),
        MAX_PAGING_SELECTIONS
    );
}
