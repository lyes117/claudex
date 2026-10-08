use super::*;
use pretty_assertions::assert_eq;
use serde::ser::SerializeSeq;
use serde_json::Number;
use serde_json::Value;
use serde_json::json;

#[test]
fn exact_encoded_string_boundary_includes_json_quotes_and_escaping() {
    assert_eq!(
        check_json_budget(&"x".repeat(MAX_WORKFLOW_JSON_BYTES - 2)),
        Ok(())
    );
    assert_eq!(
        check_json_budget(&"x".repeat(MAX_WORKFLOW_JSON_BYTES - 1)),
        Err(EncodingError::Limit)
    );
    let nested = json!({"items":["\"".repeat(2500),"\"".repeat(2500)]});
    assert_eq!(check_json_budget(&nested), Err(EncodingError::Limit));
}

#[test]
fn exact_numeric_lexemes_are_measured_without_a_serialized_buffer() {
    let exact: Number = "7".repeat(MAX_WORKFLOW_JSON_BYTES).parse().unwrap();
    assert_eq!(check_json_budget(&exact), Ok(()));
    let too_long: Number = "7".repeat(MAX_WORKFLOW_JSON_BYTES + 1).parse().unwrap();
    assert_eq!(check_json_budget(&too_long), Err(EncodingError::Limit));
    let precise: Number = "1.0000000000000000000000000000000000000000001"
        .parse()
        .unwrap();
    assert_eq!(check_json_budget(&precise), Ok(()));
    assert_eq!(
        precise.as_str(),
        "1.0000000000000000000000000000000000000000001"
    );
}

#[test]
fn four_individually_valid_results_have_one_exact_collective_budget() {
    let mut values = vec![Value::String("x".repeat(2045)); 3];
    values.push(Value::String("x".repeat(2044)));
    for value in &values {
        assert_eq!(check_json_budget(value), Ok(()));
    }
    assert_eq!(check_json_budget(&values), Ok(())); // 8192 including []/quotes/commas.
    values[3] = Value::String("x".repeat(2045));
    assert_eq!(check_json_budget(&values), Err(EncodingError::Limit));
}

#[test]
fn counter_refuses_oversized_writes_before_advancing_or_storing_payload() {
    let mut counter = BoundedCounter {
        written: 0,
        limit: MAX_WORKFLOW_JSON_BYTES,
        exceeded: false,
    };
    counter
        .write_all(&vec![b'x'; MAX_WORKFLOW_JSON_BYTES - 1])
        .unwrap();
    assert!(
        counter
            .write_all(&vec![b'x'; MAX_WORKFLOW_JSON_BYTES * 4])
            .is_err()
    );
    assert_eq!(
        (counter.written, counter.exceeded),
        (MAX_WORKFLOW_JSON_BYTES - 1, true)
    );
}

#[test]
fn serializer_stops_producing_sequence_items_as_soon_as_budget_is_exhausted() {
    struct Streaming<'a>(&'a std::cell::Cell<usize>);
    impl Serialize for Streaming<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(None)?;
            for _ in 0..1000 {
                self.0.set(self.0.get() + 1);
                sequence.serialize_element(&"x".repeat(1024))?;
            }
            sequence.end()
        }
    }
    let produced = std::cell::Cell::new(0);
    assert_eq!(
        check_json_budget(&Streaming(&produced)),
        Err(EncodingError::Limit)
    );
    assert!(produced.get() < 10); // A full to_vec pass would have visited 1000.
}

#[test]
fn genuine_serializer_failure_remains_distinct_from_budget_exhaustion() {
    struct Invalid;
    impl Serialize for Invalid {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("synthetic invalid data"))
        }
    }
    assert_eq!(check_json_budget(&Invalid), Err(EncodingError::Invalid));
}
