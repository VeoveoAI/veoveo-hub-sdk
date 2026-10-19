//! Internal JSON resource serialization and native completion helpers.

use rmcp::{
    ErrorData,
    model::{CompleteResult, CompletionInfo, ReadResourceResult, ResourceContents},
};
use serde::Serialize;

/// A JSON resource body for `uri`.
pub fn json_read<T: Serialize + ?Sized>(
    uri: &str,
    value: &T,
) -> Result<ReadResourceResult, ErrorData> {
    let text = serde_json::to_string(value)
        .map_err(|_| ErrorData::internal_error("resource serialization failed", None))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type("application/json"),
    ]))
}

/// A completion result from ranked candidates: at most
/// [`CompletionInfo::MAX_VALUES`] values. When candidates exceed that, the
/// result sets `has_more` and omits `total`, because a bounded query that
/// returned the candidates does not know the full count.
pub fn completion(mut values: Vec<String>) -> Result<CompleteResult, ErrorData> {
    let has_more = values.len() > CompletionInfo::MAX_VALUES;
    let total = if has_more {
        None
    } else {
        u32::try_from(values.len()).ok()
    };
    values.truncate(CompletionInfo::MAX_VALUES);
    let info = CompletionInfo::with_pagination(values, total, has_more)
        .map_err(|error| ErrorData::internal_error(error, None))?;
    Ok(CompleteResult::new(info))
}

/// Ranks candidates for a completion prefix: case-insensitive prefix matches
/// first, then substring matches, each in input order.
pub fn rank_completions<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    value: &str,
) -> Vec<String> {
    let needle = value.to_lowercase();
    let (mut prefixed, mut contained) = (Vec::new(), Vec::new());
    for candidate in candidates {
        let folded = candidate.to_lowercase();
        if folded.starts_with(&needle) {
            prefixed.push(candidate.to_owned());
        } else if folded.contains(&needle) {
            contained.push(candidate.to_owned());
        }
    }
    prefixed.extend(contained);
    prefixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completions_rank_prefixes_first_and_report_only_known_totals() {
        assert_eq!(
            rank_completions(["design", "agents", "redesign"], "des"),
            ["design", "redesign"]
        );
        let many = (0..150).map(|i| format!("v{i}")).collect();
        let result = completion(many).unwrap();
        assert_eq!(result.completion.values.len(), CompletionInfo::MAX_VALUES);
        assert_eq!(result.completion.total, None);
        assert_eq!(result.completion.has_more, Some(true));
        let exact = completion(vec!["design".into()]).unwrap().completion;
        assert_eq!(exact.total, Some(1));
        assert_eq!(exact.has_more, Some(false));
    }
}
