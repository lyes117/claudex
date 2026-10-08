//! Bounded, explicit JSON Schema subset for the future native workflow bridge.
//! Unsupported assertions fail before child admission; this is not a general validator.
//! Numeric comparisons preserve exact decimal precision within bounded exponents.

#[path = "workflow_numeric.rs"]
mod numeric;

#[path = "workflow_serialization.rs"]
mod serialization;

pub(super) use serialization::EncodingError;
pub(super) use serialization::check_json_budget;

use numeric::Decimal;

use serde::Deserialize;
use serde::Deserializer;
use serde::de::DeserializeSeed;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;

pub(super) const MAX_WORKFLOW_JSON_BYTES: usize = 8192;
const MAX_NODES: usize = 256;
const MAX_DEPTH: usize = 12;

pub(super) struct WorkflowSchema {
    wire: Value,
    shape: Shape,
}

struct Shape {
    kind: Kind,
    enumeration: Option<Vec<Value>>,
}

enum Kind {
    Object {
        properties: BTreeMap<String, Shape>,
        required: HashSet<String>,
        additional: bool,
    },
    Array(Box<Shape>),
    String,
    Number,
    Integer,
    Boolean,
    Null,
}

impl WorkflowSchema {
    pub(super) fn compile(wire: Value) -> Result<Self, String> {
        Self::compile_mode(wire, false)
    }

    pub(super) fn compile_compatible(wire: Value) -> Result<Self, String> {
        Self::compile_mode(wire, true)
    }

    fn compile_mode(wire: Value, compatible: bool) -> Result<Self, String> {
        // Check the supplied AST before recursive serialization or enum cloning.
        let mut pending = vec![(&wire, 0)];
        let mut remaining = MAX_NODES;
        while let Some((value, depth)) = pending.pop() {
            consume_budget(depth, &mut remaining)?;
            match value {
                Value::Array(values) if values.len() <= remaining => {
                    pending.extend(values.iter().map(|value| (value, depth + 1)));
                }
                Value::Object(values) if values.len() <= remaining => {
                    if values.keys().any(|key| key.len() > MAX_WORKFLOW_JSON_BYTES) {
                        return Err("workflow schema key exceeds byte budget".into());
                    }
                    pending.extend(values.values().map(|value| (value, depth + 1)));
                }
                Value::Array(_) | Value::Object(_) => {
                    return Err("workflow schema exceeds node budget".into());
                }
                Value::String(text) if text.len() > MAX_WORKFLOW_JSON_BYTES => {
                    return Err("workflow schema exceeds byte budget".into());
                }
                Value::Number(number) => {
                    Decimal::from_number(number)?;
                }
                Value::Null | Value::Bool(_) | Value::String(_) => {}
            }
        }
        check_json_budget(&wire).map_err(|error| match error {
            EncodingError::Limit => "workflow schema exceeds 8192 bytes",
            EncodingError::Invalid => "invalid workflow schema",
        })?;
        let mut remaining = MAX_NODES;
        let shape = compile_shape(&wire, /*depth*/ 0, &mut remaining, compatible)?;
        Ok(Self { wire, shape })
    }

    pub(super) fn wire(&self) -> &Value {
        &self.wire
    }

    pub(super) fn compile_for_output(wire: Value) -> Result<Self, String> {
        let schema = Self::compile(wire)?;
        if !matches!(&schema.shape.kind, Kind::Object { .. }) {
            return Err("structured workflow output requires an object root".into());
        }
        let mut pending = vec![&schema.shape];
        while let Some(shape) = pending.pop() {
            match &shape.kind {
                Kind::Object {
                    properties,
                    required,
                    additional,
                } => {
                    if *additional || properties.len() != required.len() {
                        return Err("structured workflow output requires every property".into());
                    }
                    pending.extend(properties.values());
                }
                Kind::Array(items) => pending.push(items),
                Kind::String | Kind::Number | Kind::Integer | Kind::Boolean | Kind::Null => {}
            }
        }
        Ok(schema)
    }

    pub(super) fn parse_result(&self, text: &str) -> Result<Value, String> {
        if text.len() > MAX_WORKFLOW_JSON_BYTES {
            return Err("workflow result exceeds 8192 bytes".into());
        }
        let mut remaining = MAX_NODES;
        let mut decoder = serde_json::Deserializer::from_str(text);
        let result = BoundedJson {
            depth: 0,
            remaining: &mut remaining,
        }
        .deserialize(&mut decoder)
        .map_err(|_| "invalid or unbounded workflow result JSON")?;
        decoder.end().map_err(|_| "trailing workflow result JSON")?;
        remaining = MAX_NODES;
        validate_shape(&self.shape, &result, /*depth*/ 0, &mut remaining)?;
        Ok(result)
    }
}

fn consume_budget(depth: usize, remaining: &mut usize) -> Result<(), String> {
    if depth > MAX_DEPTH || *remaining == 0 {
        return Err("workflow JSON exceeds depth or node budget".into());
    }
    *remaining -= 1;
    Ok(())
}

fn compile_shape(
    value: &Value,
    depth: usize,
    remaining: &mut usize,
    compatible: bool,
) -> Result<Shape, String> {
    consume_budget(depth, remaining)?;
    let object = value
        .as_object()
        .ok_or("workflow schema must be an object")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("schema requires one type")?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "type" | "enum" => true,
            "description" => value.is_string(),
            "properties" | "required" | "additionalProperties" => kind == "object",
            "items" => kind == "array",
            _ => false,
        };
        if !valid {
            return Err("unsupported workflow schema keyword or annotation".into());
        }
    }
    let enumeration = object
        .get("enum")
        .map(|values| {
            let values = values
                .as_array()
                .filter(|values| !values.is_empty())
                .ok_or("schema enum must be a nonempty array")?;
            if values.iter().enumerate().any(|(index, value)| {
                values[..index]
                    .iter()
                    .any(|previous| json_equal(previous, value))
            }) {
                return Err("duplicate schema enum values");
            }
            Ok(values.clone())
        })
        .transpose()
        .map_err(str::to_string)?;
    let kind = match kind {
        "object" => {
            let additional = match object.get("additionalProperties") {
                Some(Value::Bool(value)) => *value,
                None if compatible => true,
                None => true,
                Some(_) => return Err("unsupported workflow additionalProperties schema".into()),
            };
            if additional && !compatible {
                return Err("workflow object schemas require additionalProperties=false".into());
            }
            let properties = object
                .get("properties")
                .and_then(Value::as_object)
                .ok_or("object schema requires properties")?;
            let mut compiled = BTreeMap::new();
            for (name, shape) in properties {
                compiled.insert(
                    name.clone(),
                    compile_shape(shape, depth + 1, remaining, compatible)?,
                );
            }
            let mut required = HashSet::new();
            if let Some(names) = object.get("required") {
                for name in names.as_array().ok_or("required must be an array")? {
                    let name = name.as_str().ok_or("required must contain strings")?;
                    if !compiled.contains_key(name) || !required.insert(name.to_string()) {
                        return Err("unknown or duplicate required property".into());
                    }
                }
            }
            Kind::Object {
                properties: compiled,
                required,
                additional,
            }
        }
        "array" => Kind::Array(Box::new(compile_shape(
            object.get("items").ok_or("array schema requires items")?,
            depth + 1,
            remaining,
            compatible,
        )?)),
        "string" => Kind::String,
        "number" => Kind::Number,
        "integer" => Kind::Integer,
        "boolean" => Kind::Boolean,
        "null" => Kind::Null,
        _ => return Err("unsupported workflow schema type".into()),
    };
    let mut shape = Shape {
        kind,
        enumeration: None,
    };
    if let Some(values) = &enumeration {
        for value in values {
            let mut budget = MAX_NODES;
            validate_shape(&shape, value, /*depth*/ 0, &mut budget)
                .map_err(|_| "schema enum contradicts its shape")?;
        }
    }
    shape.enumeration = enumeration;
    Ok(shape)
}

fn validate_shape(
    shape: &Shape,
    value: &Value,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), String> {
    consume_budget(depth, remaining)?;
    if shape
        .enumeration
        .as_ref()
        .is_some_and(|values| !values.iter().any(|allowed| json_equal(allowed, value)))
    {
        return Err("workflow result does not match enum".into());
    }
    let valid = match &shape.kind {
        Kind::Object {
            properties,
            required,
            additional,
        } => {
            let value = value
                .as_object()
                .ok_or("workflow result must be an object")?;
            if required.iter().any(|name| !value.contains_key(name)) {
                return Err("workflow result is missing a required property".into());
            }
            for (name, value) in value {
                if let Some(shape) = properties.get(name) {
                    validate_shape(shape, value, depth + 1, remaining)?;
                } else if !additional {
                    return Err("workflow result contains an extra property".into());
                }
            }
            true
        }
        Kind::Array(items) => {
            for value in value.as_array().ok_or("workflow result must be an array")? {
                validate_shape(items, value, depth + 1, remaining)?;
            }
            true
        }
        Kind::String => value.is_string(),
        Kind::Number => value
            .as_number()
            .is_some_and(|number| Decimal::from_number(number).is_ok()),
        Kind::Integer => value.as_number().is_some_and(|number| {
            Decimal::from_number(number).is_ok_and(|number| number.is_integer())
        }),
        Kind::Boolean => value.is_boolean(),
        Kind::Null => value.is_null(),
    };
    if !valid {
        return Err("workflow result has the wrong type".into());
    }
    Ok(())
}

fn json_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            match (Decimal::from_number(left), Decimal::from_number(right)) {
                (Ok(left), Ok(right)) => left == right,
                _ => false,
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_equal(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, left)| right.get(key).is_some_and(|right| json_equal(left, right)))
        }
        _ => left == right,
    }
}

// Bound allocation while decoding and reject duplicate object keys instead of silently
// retaining the last value. Byte limits are checked before this decoder is constructed.
struct BoundedJson<'a> {
    depth: usize,
    remaining: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for BoundedJson<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Value, D::Error> {
        consume_budget(self.depth, self.remaining).map_err(serde::de::Error::custom)?;
        let raw = Box::<serde_json::value::RawValue>::deserialize(decoder)?;
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        match raw.get().as_bytes().first() {
            Some(b'{') => decoder
                .deserialize_map(self)
                .map_err(serde::de::Error::custom),
            Some(b'[') => decoder
                .deserialize_seq(self)
                .map_err(serde::de::Error::custom),
            _ => serde_json::from_str(raw.get()).map_err(serde::de::Error::custom),
        }
    }
}

impl<'de> Visitor<'de> for BoundedJson<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded workflow JSON")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(BoundedJson {
            depth: self.depth + 1,
            remaining: self.remaining,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate JSON key"));
            }
            let value = object.next_value_seed(BoundedJson {
                depth: self.depth + 1,
                remaining: self.remaining,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
#[path = "workflow_schema_tests.rs"]
mod tests;
