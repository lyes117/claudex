use super::*;
use pretty_assertions::assert_eq;

fn decimal(text: &str) -> Decimal {
    Decimal::from_number(&serde_json::from_str(text).unwrap()).unwrap()
}

#[test]
fn normalizes_equivalent_numbers_and_zero_without_expanding_exponents() {
    for text in ["2", "2.0", "2e0", "200e-2", "0.020e2"] {
        assert_eq!(decimal(text), decimal("2"));
    }
    for text in ["0", "-0", "-0.000e-8192", "0e8192"] {
        assert_eq!(decimal(text), decimal("0"));
    }
    assert_eq!(decimal("-1.200e3"), decimal("-1200"));
    assert_eq!(decimal("1e8192").coefficient, "1");
}

#[test]
fn distinguishes_small_fractions_underflow_and_large_adjacent_integers() {
    for (left, right) in [
        ("1", "1.0000000000000000001"),
        ("0", "1e-400"),
        ("0.1", "0.10000000000000000001"),
        ("9007199254740992", "9007199254740993"),
        ("1e8192", "1e8191"),
        ("1", "-1"),
    ] {
        assert_ne!(decimal(left), decimal(right));
    }
    assert_eq!(decimal("9007199254740993.0"), decimal("9007199254740993"));
    assert!(!decimal("1.0000000000000000001").is_integer());
    assert!(!decimal("1e-8192").is_integer());
    assert!(decimal("100e-2").is_integer());
}

#[test]
fn rejects_oversized_lexemes_and_exponents_before_normalization() {
    for text in [
        "1".repeat(super::super::MAX_WORKFLOW_JSON_BYTES + 1),
        "1e8193".into(),
        "1e-8193".into(),
        "0e999999999999999999".into(),
    ] {
        let number = serde_json::from_str::<Number>(&text).unwrap();
        assert!(Decimal::from_number(&number).is_err());
    }
}
