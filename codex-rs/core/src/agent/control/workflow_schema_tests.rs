use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn original_open_optional_schemas_keep_their_semantics_in_native_text_output() {
    let wire = json!({"type":"object","properties":{"ok":{"type":"boolean"},"note":{"type":"string"}},"required":["ok"]});
    assert!(WorkflowSchema::compile_for_output(wire.clone()).is_err());
    let schema = WorkflowSchema::compile_compatible(wire.clone()).unwrap();
    assert_eq!(schema.wire(), &wire);
    assert_eq!(
        schema
            .parse_result(r#"{"ok":true,"extra":"permitted"}"#)
            .unwrap(),
        json!({"ok":true,"extra":"permitted"})
    );
    assert!(
        schema
            .parse_result(r#"{"note":"missing required"}"#)
            .is_err()
    );
    assert!(schema.parse_result(r#"{"ok":"wrong type"}"#).is_err());
    assert!(schema.parse_result(r#"{"ok":true,"note":false}"#).is_err());
}

fn result_schema() -> Value {
    json!({"type":"object", "properties": {
        "answer":{"type":"string", "enum":["ok","no"]},
        "counts":{"type":"array","items":{"type":"integer"}},
        "optional":{"type":"boolean"}
    }, "required":["answer","counts"], "additionalProperties":false})
}

#[test]
fn validates_nested_results_and_preserves_the_wire_schema() {
    let wire = result_schema();
    let schema = WorkflowSchema::compile(wire.clone()).unwrap();
    assert_eq!(schema.wire(), &wire);
    assert_eq!(
        schema
            .parse_result(r#"{"answer":"ok","counts":[1,2.0],"optional":true}"#)
            .unwrap(),
        json!({"answer":"ok","counts":[1,2.0],"optional":true})
    );
    assert!(
        schema
            .parse_result(r#"{"answer":"no","counts":[]}"#)
            .is_ok()
    );
}

#[test]
fn rejects_each_unsupported_assertion_before_admission() {
    for keyword in [
        "$ref", "anyOf", "oneOf", "allOf", "pattern", "minimum", "maxItems", "format", "default",
    ] {
        let mut schema = result_schema();
        schema[keyword] = json!(true);
        assert!(WorkflowSchema::compile(schema).is_err(), "{keyword}");
    }
    for schema in [
        json!(true),
        json!({}),
        json!({"type":["string","null"]}),
        json!({"type":"array"}),
        json!({"type":"object","properties":{}}),
        json!({"type":"object","properties":{},"additionalProperties":true}),
        json!({"type":"string","items":{"type":"string"}}),
        json!({"type":"string","enum":[]}),
        json!({"type":"string","enum":["x","x"]}),
    ] {
        assert!(WorkflowSchema::compile(schema).is_err());
    }
}

#[test]
fn rejects_wrong_types_extra_missing_and_invalid_results() {
    let schema = WorkflowSchema::compile(result_schema()).unwrap();
    for text in [
        "not json",
        "{}",
        r#"{"answer":"ok","counts":[],"extra":1}"#,
        r#"{"answer":"wrong","counts":[]}"#,
        r#"{"answer":"ok","counts":[1.1]}"#,
        r#"{"answer":"ok","counts":[true]}"#,
        r#"{"answer":"ok","counts":[],"optional":1}"#,
        r#"{"answer":"ok","counts":[1.0000000000000000001]}"#,
        r#"{"answer":"wrong","answer":"ok","counts":[]}"#,
        r#"{"answer":"ok","counts":[]} {}"#,
    ] {
        assert!(schema.parse_result(text).is_err(), "{text}");
    }
}

#[test]
fn numeric_enum_compares_exact_decimals_without_rounding_large_integers() {
    let schema =
        WorkflowSchema::compile(json!({"type":"integer","enum":[2,9007199254740993u64]})).unwrap();
    assert_eq!(schema.parse_result("2.0").unwrap(), json!(2.0));
    assert!(schema.parse_result("9007199254740992").is_err());
    assert_eq!(
        schema.parse_result("9007199254740993").unwrap(),
        json!(9007199254740993u64)
    );
    assert!(WorkflowSchema::compile(json!({"type":"number","enum":[2,2.0]})).is_err());
}

#[test]
fn numeric_results_preserve_precision_for_integer_and_nested_enum_validation() {
    let integer = WorkflowSchema::compile(json!({"type":"integer"})).unwrap();
    for text in ["1.0000000000000000001", "1e-400", "0.10000000000000000001"] {
        assert!(integer.parse_result(text).is_err(), "{text}");
    }
    for text in ["2", "2.0", "2e0", "200e-2", "-0", "9007199254740993.0"] {
        assert!(integer.parse_result(text).is_ok(), "{text}");
    }
    for (allowed, rejected) in [
        ("1", "1.0000000000000000001"),
        ("0", "1e-400"),
        ("0.1", "0.10000000000000000001"),
        ("9007199254740992", "9007199254740993"),
    ] {
        let wire =
            format!(r#"{{"type":"array","items":{{"type":"number"}},"enum":[[{allowed}]]}}"#);
        let schema = WorkflowSchema::compile(serde_json::from_str(&wire).unwrap()).unwrap();
        assert!(schema.parse_result(&format!("[{rejected}]")).is_err());
        assert!(schema.parse_result(&format!("[{allowed}]")).is_ok());
    }
    for equivalents in ["[2,2.0]", "[2,2e0]", "[2,200e-2]", "[-0,0]"] {
        let wire = format!(r#"{{"type":"number","enum":{equivalents}}}"#);
        assert!(WorkflowSchema::compile(serde_json::from_str(&wire).unwrap()).is_err());
    }
}

#[test]
fn numeric_exponent_budget_applies_to_results_and_schema_enumerations() {
    let number = WorkflowSchema::compile(json!({"type":"number"})).unwrap();
    for text in ["1e8192", "1e-8192", "0e8192"] {
        assert!(number.parse_result(text).is_ok(), "{text}");
    }
    for text in ["1e8193", "1e-8193", "0e8193", "1e999999999999999999"] {
        assert!(number.parse_result(text).is_err(), "{text}");
        let wire = format!(r#"{{"type":"number","enum":[{text}]}}"#);
        assert!(WorkflowSchema::compile(serde_json::from_str(&wire).unwrap()).is_err());
    }
}

#[test]
fn strict_output_rejects_primitive_roots_optional_properties_and_contradictory_enums() {
    assert!(WorkflowSchema::compile_for_output(json!({"type":"string"})).is_err());
    assert!(WorkflowSchema::compile_for_output(result_schema()).is_err());
    let mut wire = result_schema();
    wire["required"] = json!(["answer", "counts", "optional"]);
    assert!(WorkflowSchema::compile_for_output(wire.clone()).is_ok());
    wire["properties"]["counts"]["items"] = json!({"type":"object","properties":{"nested":{"type":"string"}},"additionalProperties":false});
    assert!(WorkflowSchema::compile_for_output(wire).is_err());
    assert!(WorkflowSchema::compile(json!({"type":"string","enum":[false]})).is_err());
}

#[test]
fn arbitrary_precision_internal_number_key_does_not_retype_an_object() {
    let integer = WorkflowSchema::compile(json!({"type":"integer"})).unwrap();
    assert!(
        integer
            .parse_result(r#"{"$serde_json::private::Number":"2"}"#)
            .is_err()
    );
    let object = WorkflowSchema::compile(json!({"type":"object","properties":{
        "$serde_json::private::Number":{"type":"string"}}, "required":["$serde_json::private::Number"], "additionalProperties":false})).unwrap();
    assert_eq!(
        object
            .parse_result(r#"{"$serde_json::private::Number":"2"}"#)
            .unwrap(),
        json!({"$serde_json::private::Number":"2"})
    );
}

#[test]
fn rejects_unknown_duplicate_required_and_nested_assertions() {
    for required in [
        json!(["answer", "answer"]),
        json!(["missing"]),
        json!([1]),
        json!(false),
    ] {
        let mut wire = result_schema();
        wire["required"] = required;
        assert!(WorkflowSchema::compile(wire).is_err());
    }
    let mut wire = result_schema();
    wire["properties"]["answer"]["maxLength"] = json!(10);
    assert!(WorkflowSchema::compile(wire).is_err());
}

#[test]
fn enforces_schema_and_result_byte_depth_and_node_budgets() {
    let string = WorkflowSchema::compile(json!({"type":"string"})).unwrap();
    let at_limit = format!("\"{}\"", "a".repeat(MAX_WORKFLOW_JSON_BYTES - 2));
    assert!(string.parse_result(&at_limit).is_ok());
    assert!(string.parse_result(&(at_limit + " ")).is_err());
    assert!(
        WorkflowSchema::compile(
            json!({"type":"string","description":"x".repeat(MAX_WORKFLOW_JSON_BYTES)})
        )
        .is_err()
    );
    let mut deep = json!({"type":"string"});
    for _ in 0..MAX_DEPTH + 1 {
        deep = json!({"type":"array","items":deep});
    }
    assert!(WorkflowSchema::compile(deep).is_err());
    let array = WorkflowSchema::compile(json!({"type":"array","items":{"type":"null"}})).unwrap();
    assert!(
        array
            .parse_result(&json!(vec![Value::Null; MAX_NODES - 1]).to_string())
            .is_ok()
    );
    assert!(
        array
            .parse_result(&json!(vec![Value::Null; MAX_NODES]).to_string())
            .is_err()
    );
}

#[test]
fn byte_writer_does_not_change_schema_depth_or_numeric_identity() {
    let mut at_depth = json!({"type":"string"});
    for _ in 0..MAX_DEPTH - 1 {
        at_depth = json!({"type":"array","items":at_depth});
    }
    let compiled = WorkflowSchema::compile(at_depth.clone()).unwrap();
    assert_eq!(compiled.wire(), &at_depth);
    let too_deep = json!({"type":"array","items":at_depth});
    assert!(WorkflowSchema::compile(too_deep).is_err());

    let precise: serde_json::Number = "1.0000000000000000000000000000000000000000001"
        .parse()
        .unwrap();
    let wire = json!({"type":"number","enum":[precise]});
    let compiled = WorkflowSchema::compile(wire.clone()).unwrap();
    assert_eq!(compiled.wire(), &wire);
    assert!(
        compiled
            .parse_result("1.0000000000000000000000000000000000000000001")
            .is_ok()
    );
    assert!(compiled.parse_result("1.0").is_err());
}

#[test]
fn schema_wire_encoding_budget_includes_nested_escaped_strings_and_numbers() {
    let nested = json!({"type":"object","properties":{
        "first":{"type":"string","description":"\"".repeat(2500)},
        "second":{"type":"string","description":"\"".repeat(2500)}
    },"required":["first","second"],"additionalProperties":false});
    assert_eq!(
        WorkflowSchema::compile(nested).err(),
        Some("workflow schema exceeds 8192 bytes".into())
    );
    let number: serde_json::Number = "7".repeat(MAX_WORKFLOW_JSON_BYTES).parse().unwrap();
    let wire = json!({"type":"number","enum":[number]});
    assert_eq!(
        WorkflowSchema::compile(wire).err(),
        Some("workflow schema exceeds 8192 bytes".into())
    );
}
