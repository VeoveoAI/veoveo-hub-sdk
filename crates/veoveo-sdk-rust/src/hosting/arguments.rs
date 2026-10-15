//! The explanation of a refused tool call. The tool's own decoding refuses arguments
//! with its owner's message, which names Rust types. `Hosted` then appends the
//! violations of the tool's published input schema: each argument's JSON Pointer and the
//! type or shape the schema expects, without the caller's values.

use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
use serde_json::Value;

/// The most violations one explanation lists.
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
                    // then a refusal carries the decoding's message alone.
                    tracing::warn!(tool = %tool.name, %error, "tool input schema does not compile");
                    None
                }
            }
        })
        .clone()
}

/// The violations of `tool`'s published input schema by `arguments`, or `None` when the
/// schema admits them.
fn violations<D: 'static>(tool: &Tool, arguments: Option<&JsonObject>) -> Option<String> {
    let validator = validator::<D>(tool)?;
    let instance = Value::Object(arguments.cloned().unwrap_or_default());
    let mut violations: Vec<String> = Vec::new();
    for error in validator.iter_errors(&instance) {
        let path = error.instance_path().to_string();
        let at = if path.is_empty() {
            "the arguments".to_owned()
        } else {
            format!("`{path}`")
        };
        // An argument's own `type` and its definition's report the same violation.
        let line = format!("- {at}: {}", error.masked());
        if !violations.contains(&line) {
            violations.push(line);
        }
        if violations.len() == MAX_VIOLATIONS {
            break;
        }
    }
    (!violations.is_empty()).then(|| {
        format!(
            "The arguments do not match the published input schema of `{}`:\n{}",
            tool.name,
            violations.join("\n")
        )
    })
}

/// Appends the published schema's violations to `refused`, the error result of a call
/// whose arguments the tool's decoding refused. `tool` is the tool as discovery
/// publishes it.
pub(super) fn explain<D: 'static>(
    tool: &Tool,
    arguments: Option<&JsonObject>,
    refused: &mut CallToolResult,
) {
    if let Some(text) = violations::<D>(tool, arguments) {
        refused.content.push(ContentBlock::text(text));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::hosting::publish_input_schema;

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

    /// The tool as discovery publishes it.
    fn tool() -> Tool {
        let schema = schemars::generate::SchemaSettings::draft2020_12()
            .into_generator()
            .into_root_schema_for::<Arguments>();
        let Value::Object(mut schema) = serde_json::to_value(schema).unwrap() else {
            unreachable!()
        };
        publish_input_schema(&mut schema);
        Tool::new("resolve", "Resolve an expression.", Arc::new(schema))
    }

    fn object(value: Value) -> JsonObject {
        let Value::Object(object) = value else {
            unreachable!()
        };
        object
    }

    #[test]
    fn admitted_arguments_need_no_explanation() {
        let arguments = object(json!({
            "expression": {"format": "window", "value": {"start": "09:00", "minutes": 30}},
            "limit": 5
        }));
        assert!(violations::<Arguments>(&tool(), Some(&arguments)).is_none());
    }

    #[test]
    fn an_explanation_names_each_argument_by_its_pointer_and_type_and_hides_its_value() {
        let arguments = object(json!({
            "expression": "{\"format\": \"rfc3339\", \"value\": \"secret-value\"}",
            "limit": "20",
            "extra": true
        }));
        let text = violations::<Arguments>(&tool(), Some(&arguments)).expect("violations");
        assert!(
            text.contains("published input schema of `resolve`"),
            "{text}"
        );
        assert!(
            text.contains(r#"`/expression`: value is not of type "object""#),
            "{text}"
        );
        assert!(
            text.contains(r#"`/limit`: value is not of type "integer""#),
            "{text}"
        );
        assert!(text.contains("the arguments"), "{text}");
        assert_eq!(
            text.matches(r#"`/expression`: value is not of type "object""#)
                .count(),
            1,
            "{text}"
        );
        assert!(!text.contains("secret-value"), "{text}");
        assert!(!text.contains("Expression"), "{text}");
    }

    #[test]
    fn the_explanation_follows_the_decodings_own_refusal() {
        let mut refused = CallToolResult::error(vec![ContentBlock::text(
            "failed to deserialize parameters: missing field `expression`",
        )]);
        explain::<Arguments>(&tool(), None, &mut refused);
        assert_eq!(refused.content.len(), 2);
        assert!(
            refused.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("missing field")
        );
        assert!(
            refused.content[1]
                .as_text()
                .unwrap()
                .text
                .contains("expression")
        );
    }
}
