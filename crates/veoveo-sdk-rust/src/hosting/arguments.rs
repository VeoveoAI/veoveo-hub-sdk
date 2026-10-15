//! The check of a tool call's arguments against the tool's generated input schema,
//! before a Task starts or a handler runs. A refusal names each violation by the JSON
//! Pointer of the argument and the JSON type or shape the schema expects, never by the
//! Rust type that would have refused it, and it leaves the caller's values out.

use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
use serde_json::Value;

/// The most violations one refusal lists.
const MAX_VIOLATIONS: usize = 8;

type Validators = HashMap<(TypeId, String), Option<Arc<jsonschema::Validator>>>;

/// One compiled validator per server type and tool, for the life of the process.
fn validator<D: 'static>(tool: &Tool) -> Option<Arc<jsonschema::Validator>> {
    static VALIDATORS: OnceLock<Mutex<Validators>> = OnceLock::new();
    let key = (TypeId::of::<D>(), tool.name.to_string());
    let mut validators = VALIDATORS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the validator cache lock is never poisoned");
    validators
        .entry(key)
        .or_insert_with(|| {
            let schema = Value::Object((*tool.input_schema).clone());
            match jsonschema::validator_for(&schema) {
                Ok(validator) => Some(Arc::new(validator)),
                Err(error) => {
                    // Conformance refuses a schema that is not valid JSON Schema; until
                    // then the tool's own decoding is the only check.
                    tracing::warn!(tool = %tool.name, %error, "tool input schema does not compile");
                    None
                }
            }
        })
        .clone()
}

/// The error result that refuses `arguments`, or `None` when the schema admits them.
pub(super) fn refusal<D: 'static>(
    tool: &Tool,
    arguments: Option<&JsonObject>,
) -> Option<CallToolResult> {
    let validator = validator::<D>(tool)?;
    let instance = Value::Object(arguments.cloned().unwrap_or_default());
    let violations: Vec<String> = validator
        .iter_errors(&instance)
        .take(MAX_VIOLATIONS)
        .map(|error| {
            let path = error.instance_path().to_string();
            let at = if path.is_empty() {
                "the arguments".to_owned()
            } else {
                format!("`{path}`")
            };
            format!("- {at}: {}", error.masked())
        })
        .collect();
    if violations.is_empty() {
        return None;
    }
    Some(CallToolResult::error(vec![ContentBlock::text(format!(
        "The arguments do not match the input schema of `{}`:\n{}",
        tool.name,
        violations.join("\n")
    ))]))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Window {
        start: String,
        minutes: u32,
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(tag = "format", content = "value", rename_all = "snake_case")]
    #[allow(dead_code)]
    enum Expression {
        Rfc3339(String),
        Window(Window),
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Arguments {
        expression: Expression,
        limit: Option<u32>,
    }

    fn tool() -> Tool {
        let schema = schemars::generate::SchemaSettings::draft2020_12()
            .into_generator()
            .into_root_schema_for::<Arguments>();
        let Value::Object(schema) = serde_json::to_value(schema).unwrap() else {
            unreachable!()
        };
        Tool::new("resolve", "Resolve an expression.", Arc::new(schema))
    }

    fn text(result: &CallToolResult) -> &str {
        &result.content[0].as_text().expect("text content").text
    }

    #[test]
    fn admitted_arguments_reach_the_tool() {
        for arguments in [
            json!({"expression": {"format": "rfc3339", "value": "2026-10-15T09:00:00Z"}}),
            json!({"expression": {"format": "window", "value": {"start": "09:00", "minutes": 30}},
                "limit": null}),
        ] {
            let Value::Object(arguments) = arguments else {
                unreachable!()
            };
            assert!(refusal::<Arguments>(&tool(), Some(&arguments)).is_none());
        }
    }

    #[test]
    fn a_refusal_names_each_argument_by_its_pointer_and_hides_its_value() {
        let Value::Object(arguments) = json!({
            "expression": "{\"format\": \"rfc3339\", \"value\": \"secret-value\"}",
            "limit": "20",
            "extra": true
        }) else {
            unreachable!()
        };
        let refused = refusal::<Arguments>(&tool(), Some(&arguments)).expect("refused");
        assert_eq!(refused.is_error, Some(true));
        let text = text(&refused);
        assert!(text.contains("input schema of `resolve`"), "{text}");
        assert!(text.contains("`/expression`"), "{text}");
        assert!(text.contains("`/limit`"), "{text}");
        assert!(text.contains("the arguments"), "{text}");
        assert!(!text.contains("secret-value"), "{text}");
        assert!(!text.contains("Expression"), "{text}");
    }

    #[test]
    fn missing_arguments_are_checked_as_an_empty_object() {
        let refused = refusal::<Arguments>(&tool(), None).expect("refused");
        assert!(text(&refused).contains("expression"), "{}", text(&refused));
    }
}
