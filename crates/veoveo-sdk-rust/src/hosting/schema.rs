//! Declared JSON types in published tool input schemas.
//!
//! Schemars writes a reference to a shared definition, and a Rust enum as a `oneOf` of
//! its variants, without a `type` beside them. A model's tool-call parser that decodes
//! each argument by its declared `type` then passes an object argument on as text.
//! [`declare_types`] adds the `type` that each subschema's references, composition,
//! `const` and `enum` already imply. The schema accepts the same instances, and every
//! subschema names its JSON types where such a parser reads them.

use std::collections::HashMap;

use serde_json::{Map, Value};

/// A subschema whose instances can only have some JSON types but that declares none.
#[derive(Debug, Clone, PartialEq)]
pub struct UndeclaredType {
    /// The subschema's JSON Pointer, RFC 6901, in the schema document.
    pub pointer: String,
    /// The `type` value its references, composition, `const` and `enum` imply.
    pub types: Value,
}

/// Every subschema of `schema` that lacks `type` although its instance types are
/// restricted, in document order.
pub fn undeclared_types(schema: &Value) -> Vec<UndeclaredType> {
    let mut inference = Inference::new(schema);
    let mut undeclared = Vec::new();
    collect(schema, String::new(), &mut inference, &mut undeclared);
    undeclared
}

/// Declares `type` on every subschema of `schema` that [`undeclared_types`] reports.
pub fn declare_types(schema: &mut Map<String, Value>) {
    let mut document = Value::Object(std::mem::take(schema));
    for declaration in undeclared_types(&document) {
        if let Some(Value::Object(subschema)) = document.pointer_mut(&declaration.pointer) {
            subschema.insert("type".to_owned(), declaration.types);
        }
    }
    let Value::Object(document) = document else {
        unreachable!("the document was built from an object")
    };
    *schema = document;
}

fn collect<'a>(
    schema: &'a Value,
    pointer: String,
    inference: &mut Inference<'a>,
    undeclared: &mut Vec<UndeclaredType>,
) {
    let Value::Object(object) = schema else {
        return;
    };
    if !object.contains_key("type")
        && let Some(types) = inference.types(schema).declaration()
    {
        undeclared.push(UndeclaredType {
            pointer: pointer.clone(),
            types,
        });
    }
    for (location, child) in subschemas(object) {
        collect(child, format!("{pointer}{location}"), inference, undeclared);
    }
}

/// Each schema-valued keyword of `schema` with its relative JSON Pointer. Annotation
/// and instance values, such as `default`, `examples`, `const` and `enum`, hold
/// ordinary JSON and are not visited.
fn subschemas(schema: &Map<String, Value>) -> Vec<(String, &Value)> {
    let mut children = Vec::new();
    for keyword in [
        "properties",
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
        "dependencies",
    ] {
        if let Some(Value::Object(map)) = schema.get(keyword) {
            for (name, child) in map {
                // A Draft 7 `dependencies` entry may list property names instead.
                if !child.is_array() {
                    children.push((format!("/{keyword}/{}", token(name)), child));
                }
            }
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems", "items"] {
        if let Some(Value::Array(branches)) = schema.get(keyword) {
            for (index, child) in branches.iter().enumerate() {
                children.push((format!("/{keyword}/{index}"), child));
            }
        }
    }
    for keyword in [
        "items",
        "additionalItems",
        "additionalProperties",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contains",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
        "contentSchema",
    ] {
        if let Some(child) = schema.get(keyword).filter(|child| !child.is_array()) {
            children.push((format!("/{keyword}"), child));
        }
    }
    children
}

/// One JSON Pointer reference token, escaped as RFC 6901 requires.
fn token(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// A set of JSON Schema instance types. `number` includes `integer`, so a set with
/// `number` always holds `integer` too and intersections stay exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Types(u8);

impl Types {
    const NULL: u8 = 1;
    const BOOLEAN: u8 = 1 << 1;
    const OBJECT: u8 = 1 << 2;
    const ARRAY: u8 = 1 << 3;
    const STRING: u8 = 1 << 4;
    const INTEGER: u8 = 1 << 5;
    const NUMBER: u8 = 1 << 6 | Self::INTEGER;
    const NONE: Self = Self(0);
    const ANY: Self =
        Self(Self::NULL | Self::BOOLEAN | Self::OBJECT | Self::ARRAY | Self::STRING | Self::NUMBER);
    /// Declaration order: the type names sorted, so a declaration is deterministic.
    const NAMES: [(u8, &'static str); 7] = [
        (Self::ARRAY, "array"),
        (Self::BOOLEAN, "boolean"),
        (Self::INTEGER, "integer"),
        (Self::NULL, "null"),
        (Self::NUMBER, "number"),
        (Self::OBJECT, "object"),
        (Self::STRING, "string"),
    ];

    fn named(name: &str) -> Self {
        Self::NAMES
            .iter()
            .find(|(_, named)| *named == name)
            // An unknown name is an invalid schema, which restricts nothing here.
            .map_or(Self::ANY, |(bits, _)| Self(*bits))
    }

    fn of(value: &Value) -> Self {
        Self(match value {
            Value::Null => Self::NULL,
            Value::Bool(_) => Self::BOOLEAN,
            Value::Number(number)
                if number.is_i64()
                    || number.is_u64()
                    || number.as_f64().is_some_and(|float| float.fract() == 0.0) =>
            {
                Self::INTEGER
            }
            Value::Number(_) => Self::NUMBER,
            Value::String(_) => Self::STRING,
            Value::Array(_) => Self::ARRAY,
            Value::Object(_) => Self::OBJECT,
        })
    }

    fn and(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The `type` value for a restricted, satisfiable set.
    fn declaration(self) -> Option<Value> {
        if self == Self::ANY || self == Self::NONE {
            return None;
        }
        let mut names: Vec<Value> = Self::NAMES
            .iter()
            .filter(|(bits, name)| {
                self.0 & bits == *bits
                    // `number` already admits every integer.
                    && !(*name == "integer" && self.0 & Self::NUMBER == Self::NUMBER)
            })
            .map(|(_, name)| Value::from(*name))
            .collect();
        Some(if names.len() == 1 {
            names.remove(0)
        } else {
            Value::Array(names)
        })
    }
}

/// Instance types inferred over one schema document. Each set contains every type
/// the subschema admits, so declaring it never rejects an instance the subschema
/// accepted. A reference already being inferred, a reference that does not resolve
/// to a JSON Pointer in the document, `not` and conditionals restrict nothing.
struct Inference<'a> {
    document: &'a Value,
    references: HashMap<&'a str, Types>,
    pending: Vec<&'a str>,
}

impl<'a> Inference<'a> {
    fn new(document: &'a Value) -> Self {
        Self {
            document,
            references: HashMap::new(),
            pending: Vec::new(),
        }
    }

    fn types(&mut self, schema: &'a Value) -> Types {
        let Value::Object(schema) = schema else {
            return if schema == &Value::Bool(false) {
                Types::NONE
            } else {
                Types::ANY
            };
        };
        let mut types = match schema.get("type") {
            Some(Value::String(name)) => Types::named(name),
            Some(Value::Array(names)) => names
                .iter()
                .map(|name| name.as_str().map_or(Types::ANY, Types::named))
                .fold(Types::NONE, Types::or),
            _ => Types::ANY,
        };
        if let Some(value) = schema.get("const") {
            types = types.and(Types::of(value));
        }
        if let Some(Value::Array(values)) = schema.get("enum") {
            types = types.and(values.iter().map(Types::of).fold(Types::NONE, Types::or));
        }
        if let Some(Value::String(reference)) = schema.get("$ref") {
            types = types.and(self.reference(reference));
        }
        if let Some(Value::Array(branches)) = schema.get("allOf") {
            for branch in branches {
                types = types.and(self.types(branch));
            }
        }
        for keyword in ["anyOf", "oneOf"] {
            if let Some(Value::Array(branches)) = schema.get(keyword) {
                let mut admitted = Types::NONE;
                for branch in branches {
                    admitted = admitted.or(self.types(branch));
                }
                types = types.and(admitted);
            }
        }
        types
    }

    fn reference(&mut self, reference: &'a str) -> Types {
        if let Some(types) = self.references.get(reference) {
            return *types;
        }
        if self.pending.contains(&reference) {
            return Types::ANY;
        }
        let Some(target) = reference
            .strip_prefix('#')
            .filter(|pointer| pointer.is_empty() || pointer.starts_with('/'))
            .and_then(|pointer| self.document.pointer(pointer))
        else {
            return Types::ANY;
        };
        self.pending.push(reference);
        let types = self.types(target);
        self.pending.pop();
        self.references.insert(reference, types);
        types
    }
}

#[cfg(test)]
mod tests {
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
    #[allow(dead_code)]
    struct Label(String);

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Arguments {
        expression: Expression,
        window: Option<Window>,
        label: Label,
        limit: Option<u32>,
        labels: Vec<Label>,
        payload: Value,
    }

    fn published() -> (Value, Value) {
        let generated = serde_json::to_value(
            schemars::generate::SchemaSettings::draft2020_12()
                .into_generator()
                .into_root_schema_for::<Arguments>(),
        )
        .unwrap();
        let Value::Object(mut schema) = generated.clone() else {
            panic!("object schema")
        };
        declare_types(&mut schema);
        (generated, Value::Object(schema))
    }

    #[test]
    fn declares_the_types_schemars_leaves_implicit_in_references_and_unions() {
        let (_, schema) = published();
        let properties = &schema["properties"];
        assert_eq!(properties["expression"]["type"], "object");
        assert_eq!(properties["expression"]["$ref"], "#/$defs/Expression");
        assert_eq!(properties["window"]["type"], json!(["null", "object"]));
        assert_eq!(properties["label"]["type"], "string");
        assert_eq!(properties["limit"]["type"], json!(["integer", "null"]));
        assert_eq!(properties["labels"]["type"], "array");
        assert_eq!(properties["labels"]["items"]["type"], "string");
        assert!(properties["payload"].get("type").is_none());
        assert_eq!(schema["$defs"]["Expression"]["type"], "object");
        assert!(undeclared_types(&schema).is_empty());
    }

    #[test]
    fn declared_types_accept_exactly_the_instances_the_generated_schema_accepts() {
        let (generated, published) = published();
        let generated = jsonschema::validator_for(&generated).unwrap();
        let published = jsonschema::validator_for(&published).unwrap();
        for instance in [
            json!({"expression":{"format":"rfc3339","value":"2026-10-15T09:00:00Z"},
                "label":"review","labels":[],"payload":null}),
            json!({"expression":{"format":"window","value":{"start":"09:00","minutes":30}},
                "window":{"start":"09:00","minutes":30},"label":"review","limit":5,
                "labels":["a"],"payload":[1]}),
            json!({"expression":"{\"format\":\"rfc3339\",\"value\":\"2026-10-15T09:00:00Z\"}",
                "label":"review","labels":[],"payload":null}),
            json!({"expression":{"format":"rfc3339","value":"x"},"label":7,"labels":[],"payload":1}),
            json!({"expression":{"format":"rfc3339","value":"x"},"label":"l","limit":1.5,
                "labels":[],"payload":1}),
            json!({"expression":{"format":"rfc3339","value":"x"},"label":"l","window":"w",
                "labels":[],"payload":1}),
        ] {
            assert_eq!(
                generated.is_valid(&instance),
                published.is_valid(&instance),
                "{instance}"
            );
        }
    }

    #[test]
    fn reports_each_restricted_subschema_by_its_pointer() {
        let schema = json!({"type":"object", "properties":{
            "a/b":{"$ref":"#/$defs/Mode"},
            "c~d":{"enum":["x", 1.5, null]},
            "flag":{"const":true},
            "any":{"anyOf":[{"type":"string"}, true]},
            "never":false
        }, "$defs":{"Mode":{"oneOf":[
            {"type":"object", "properties":{"kind":{"const":"a"}}},
            {"type":"object", "properties":{"kind":{"const":"b"}}}
        ]}}});
        let undeclared: Vec<(String, Value)> = undeclared_types(&schema)
            .into_iter()
            .map(|entry| (entry.pointer, entry.types))
            .collect();
        assert_eq!(
            undeclared,
            vec![
                ("/properties/a~1b".to_owned(), json!("object")),
                (
                    "/properties/c~0d".to_owned(),
                    json!(["null", "number", "string"])
                ),
                ("/properties/flag".to_owned(), json!("boolean")),
                ("/$defs/Mode".to_owned(), json!("object")),
                (
                    "/$defs/Mode/oneOf/0/properties/kind".to_owned(),
                    json!("string")
                ),
                (
                    "/$defs/Mode/oneOf/1/properties/kind".to_owned(),
                    json!("string")
                ),
            ]
        );
    }

    #[test]
    fn instance_payloads_and_open_references_stay_as_written() {
        let mut schema = json!({"type":"object", "properties":{
            "anchored":{"$ref":"#node"},
            "recursive":{"$ref":"#/$defs/Node"},
            "missing":{"$ref":"#/$defs/Missing"},
            "negated":{"not":{"type":"string"}}
        }, "default":{"$ref":"#/$defs/Node"}, "examples":[{"oneOf":[{"type":"string"}]}],
        "$defs":{"Node":{"anyOf":[{"$ref":"#/$defs/Node"}, {"type":"string"}]}}});
        let before = schema.clone();
        let Value::Object(object) = &mut schema else {
            unreachable!()
        };
        declare_types(object);
        assert_eq!(schema["default"], before["default"]);
        assert_eq!(schema["examples"], before["examples"]);
        for name in ["anchored", "recursive", "missing", "negated"] {
            assert!(schema["properties"][name].get("type").is_none(), "{name}");
        }
        assert_eq!(schema["properties"]["negated"]["not"]["type"], "string");
    }

    #[test]
    fn repeated_references_are_inferred_once() {
        let mut definitions = Map::new();
        definitions.insert("N0".into(), json!({"type":"object"}));
        for index in 1..64 {
            let reference = format!("#/$defs/N{}", index - 1);
            definitions.insert(
                format!("N{index}"),
                json!({"anyOf":[{"$ref":reference}, {"$ref":reference}]}),
            );
        }
        let schema = json!({"type":"object", "properties":{"node":{"$ref":"#/$defs/N63"}},
            "$defs":definitions});
        let undeclared = undeclared_types(&schema);
        // The property, 63 unions and their 126 branches.
        assert_eq!(undeclared.len(), 190);
        assert!(undeclared.iter().all(|entry| entry.types == "object"));
    }
}
