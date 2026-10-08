use super::*;
use pretty_assertions::assert_eq;

#[test]
fn bounded_expansion_keeps_arguments_unicode_escaping_and_rejects_hidden_or_oversize_commands() {
    let path = Path::new("command.md");
    let metadata = serde_json::json!({});
    let tokens = vec!["日本語 avec espaces".to_string(), "deux".to_string()];
    assert_eq!(
        expand(
            &metadata,
            "Read $0 then $ARGUMENTS[1], literal \\$ARGUMENTS",
            "raw",
            &tokens,
            path,
            8192
        )
        .expect("expanded"),
        "Read 日本語 avec espaces then deux, literal $ARGUMENTS"
    );
    assert!(
        expand(
            &serde_json::json!({"user-invocable": false}),
            "body",
            "",
            &[],
            path,
            8192
        )
        .is_err()
    );
    assert!(
        expand(
            &serde_json::json!({"context": "fork"}),
            "body",
            "",
            &[],
            path,
            8192
        )
        .is_err()
    );
    assert!(
        expand(
            &metadata,
            "$ARGUMENTS".repeat(1000).as_str(),
            &"x".repeat(8192),
            &[],
            path,
            8192
        )
        .is_err()
    );
    assert_eq!(
        expand(&metadata, "x".repeat(8192).as_str(), "", &[], path, 8192)
            .expect("exact budget")
            .len(),
        8192
    );
    assert!(expand(&metadata, "x".repeat(8193).as_str(), "", &[], path, 8192).is_err());
}
