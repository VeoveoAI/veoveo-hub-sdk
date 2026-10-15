//! Published tool input schemas that every tool-call parser reads the same way.
//!
//! Some providers turn a model's tool call into JSON arguments by decoding each argument
//! as its declared `type`. Such a parser passes an argument without a `type` on as text,
//! and it cannot read a `type` that names several types. Schemars writes both shapes:
//! a reference to a shared definition, or a Rust enum's `oneOf`, has no `type` beside
//! it, and an `Option` argument admits `null` beside its value's type.
//!
//! [`publish_input_schema`] gives each optional argument its value's type and leaves
//! `null` out, so a caller omits the argument instead. It then declares on every
//! subschema the `type` its references, composition, `const` and `enum` already imply.
//! The published schema accepts the generated schema's instances except a `null`
//! optional argument, and the server still decodes one.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

/// A subschema whose instances can only have some JSON types but that declares none.
#[derive(Debug, Clone, PartialEq)]
pub struct UndeclaredType {
    /// The subschema's JSON Pointer, RFC 6901, in the schema document.
    pub pointer: String,
    /// The `type` value its references, composition, `const` and `enum` imply.
    pub types: Value,
}

/// A tool argument whose `type` names more than one JSON type.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbiguousArgument {
    pub name: String,
    pub types: Value,
}

/// Publishes a tool input schema: an optional argument that admits `null` beside other
/// types loses `null`, and every subschema whose instance types are restricted
/// declares them.
pub fn publish_input_schema(schema: &mut Map<String, Value>) {
    omit_null_from_optional_arguments(schema);
    declare_types(schema);
}

/// Every subschema of `schema` that lacks `type` although its instance types are
/// restricted, in document order.
pub fn undeclared_types(schema: &Value) -> Vec<UndeclaredType> {
    let mut inference = Inference::new(schema);
    let mut undeclared = Vec::new();
    collect(schema, String::new(), &mut inference, &mut undeclared);
    undeclared
}

/// Every argument of the tool input `schema` that declares more than one JSON type.
pub fn ambiguous_arguments(schema: &Value) -> Vec<AmbiguousArgument> {
    let Some(Value::Object(arguments)) = schema.get("properties") else {
        return Vec::new();
    };
    arguments
        .iter()
        .filter_map(|(name, argument)| match argument.get("type") {
            Some(types @ Value::Array(_)) => Some(AmbiguousArgument {
                name: name.clone(),
                types: types.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Declares `type` on every subschema of `schema` that [`undeclared_types`] reports.
fn declare_types(schema: &mut Map<String, Value>) {
    let mut document = Value::Object(std::mem::take(schema));
    for declaration in undeclared_types(&document) {
        if let Some(Value::Object(subschema)) = document.pointer_mut(&declaration.pointer) {
            subschema.insert("type".to_owned(), declaration.types);
        }
    }
    *schema = into_object(document);
}

/// How one optional argument leaves `null` out.
struct NullOmission {
    argument: String,
    /// The argument's types without `null`.
    types: Value,
    /// The `anyOf` or `oneOf` branches that admit only `null`.
    branches: Vec<(&'static str, Vec<usize>)>,
}

fn omit_null_from_optional_arguments(schema: &mut Map<String, Value>) {
    let document = Value::Object(std::mem::take(schema));
    let omissions = null_omissions(&document);
    let mut document = into_object(document);
    let arguments = document
        .get_mut("properties")
        .and_then(Value::as_object_mut);
    if let Some(arguments) = arguments {
        for omission in omissions {
            if let Some(Value::Object(argument)) = arguments.get_mut(&omission.argument) {
                omit_null(argument, omission);
            }
        }
    }
    *schema = document;
}

fn null_omissions(document: &Value) -> Vec<NullOmission> {
    let Some(Value::Object(arguments)) = document.get("properties") else {
        return Vec::new();
    };
    let required: HashSet<&str> = document
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut inference = Inference::new(document);
    let mut omissions = Vec::new();
    for (name, argument) in arguments {
        let types = inference.types(argument);
        if required.contains(name.as_str()) || types == Types::ANY || !types.admits_null() {
            continue;
        }
        // An argument that admits only `null` keeps it.
        let Some(declaration) = types.without_null().declaration() else {
            continue;
        };
        let mut branches = Vec::new();
        for keyword in ["anyOf", "oneOf"] {
            if let Some(Value::Array(alternatives)) = argument.get(keyword) {
                let null_only: Vec<usize> = alternatives
                    .iter()
                    .enumerate()
                    .filter(|(_, alternative)| inference.types(alternative) == Types::ONLY_NULL)
                    .map(|(index, _)| index)
                    .collect();
                if !null_only.is_empty() {
                    branches.push((keyword, null_only));
                }
            }
        }
        omissions.push(NullOmission {
            argument: name.clone(),
            types: declaration,
            branches,
        });
    }
    omissions
}

/// Declares the argument's types without `null` and removes what only `null` matched:
/// a `null` member of `enum`, a `null` default and each branch that admits only `null`.
/// A composition left with one branch becomes that branch when their keywords agree.
fn omit_null(argument: &mut Map<String, Value>, omission: NullOmission) {
    argument.insert("type".to_owned(), omission.types);
    if let Some(Value::Array(values)) = argument.get_mut("enum") {
        values.retain(|value| !value.is_null());
    }
    if argument.get("default") == Some(&Value::Null) {
        argument.remove("default");
    }
    for (keyword, null_only) in omission.branches {
        if let Some(Value::Array(alternatives)) = argument.get_mut(keyword) {
            for index in null_only.into_iter().rev() {
                alternatives.remove(index);
            }
        }
        let Some(Value::Array(alternatives)) = argument.get(keyword) else {
            continue;
        };
        let [Value::Object(alternative)] = alternatives.as_slice() else {
            continue;
        };
        let agrees = alternative
            .iter()
            .all(|(key, value)| argument.get(key).is_none_or(|existing| existing == value));
        if agrees {
            let alternative = alternative.clone();
            argument.remove(keyword);
            argument.extend(alternative);
        }
    }
}

fn into_object(document: Value) -> Map<String, Value> {
    let Value::Object(document) = document else {
        unreachable!("the document was built from an object")
    };
    document
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
    const ONLY_NULL: Self = Self(Self::NULL);
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

    fn admits_null(self) -> bool {
        self.0 & Self::NULL != 0
    }

    fn without_null(self) -> Self {
        Self(self.0 & !Self::NULL)
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
        publish_input_schema(&mut schema);
        (generated, Value::Object(schema))
    }

    #[test]
    fn every_argument_schemars_generates_declares_one_type() {
        let (_, schema) = published();
        let properties = &schema["properties"];
        assert_eq!(properties["expression"]["type"], "object");
        assert_eq!(properties["expression"]["$ref"], "#/$defs/Expression");
        assert_eq!(properties["window"]["type"], "object");
        assert_eq!(properties["window"]["$ref"], "#/$defs/Window");
        assert!(properties["window"].get("anyOf").is_none());
        assert_eq!(properties["label"]["type"], "string");
        assert_eq!(properties["limit"]["type"], "integer");
        assert_eq!(properties["labels"]["type"], "array");
        assert_eq!(properties["labels"]["items"]["type"], "string");
        assert!(properties["payload"].get("type").is_none());
        assert_eq!(schema["$defs"]["Expression"]["type"], "object");
        assert!(undeclared_types(&schema).is_empty());
        assert!(ambiguous_arguments(&schema).is_empty());
    }

    #[test]
    fn the_published_schema_accepts_the_generated_instances_but_null_optional_arguments() {
        let (generated, published) = published();
        let generated = jsonschema::validator_for(&generated).unwrap();
        let published = jsonschema::validator_for(&published).unwrap();
        for instance in [
            json!({"expression":{"format":"rfc3339","value":"x"},"label":"l","window":null,
                "labels":[],"payload":1}),
            json!({"expression":{"format":"rfc3339","value":"x"},"label":"l","limit":null,
                "labels":[],"payload":1}),
        ] {
            assert!(generated.is_valid(&instance), "{instance}");
            assert!(!published.is_valid(&instance), "{instance}");
        }
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
    fn required_open_and_null_only_arguments_keep_their_types() {
        let mut schema = json!({"type":"object", "required":["at"], "properties":{
            "at":{"type":["string", "null"]},
            "either":{"type":["integer", "string"]},
            "nothing":{"type":"null"},
            "payload":true,
            "open":{},
            "note":{"anyOf":[{"type":"string"}, {"type":"null"}], "default":null,
                "description":"A note."},
            "mode":{"enum":["a", "b", null]}
        }});
        let Value::Object(object) = &mut schema else {
            unreachable!()
        };
        publish_input_schema(object);
        let arguments = &schema["properties"];
        assert_eq!(
            arguments["note"],
            json!({"type":"string", "description":"A note."})
        );
        assert_eq!(
            arguments["mode"],
            json!({"type":"string", "enum":["a", "b"]})
        );
        assert_eq!(arguments["nothing"], json!({"type":"null"}));
        assert_eq!(arguments["payload"], json!(true));
        assert_eq!(arguments["open"], json!({}));
        let ambiguous: Vec<String> = ambiguous_arguments(&schema)
            .into_iter()
            .map(|argument| argument.name)
            .collect();
        assert_eq!(ambiguous, ["at", "either"]);
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
