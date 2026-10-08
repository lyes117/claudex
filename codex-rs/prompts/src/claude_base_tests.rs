use super::*;

#[test]
fn base_instructions_are_the_claude_prompt_ending_with_the_ponytail_contract() {
    let base = claude_base_instructions();
    assert!(
        base.starts_with("You are Claude Code, Anthropic's official CLI for Claude."),
        "base instructions must open with the Claude Code marker"
    );
    assert!(
        base.ends_with(PONYTAIL_CONTRACT),
        "base instructions must end with the shared ponytail contract"
    );
    assert_eq!(
        base.matches(PONYTAIL_CONTRACT).count(),
        1,
        "the contract must appear exactly once in the base instructions"
    );
}

#[test]
fn with_ponytail_appends_the_shared_contract_once() {
    let text = with_ponytail("Role body.");
    assert!(text.starts_with("Role body.\n\n"));
    assert!(text.ends_with(PONYTAIL_CONTRACT));
    assert_eq!(text.matches(PONYTAIL_CONTRACT).count(), 1);
    assert!(PONYTAIL_CONTRACT.contains("ponytail:"));
}
