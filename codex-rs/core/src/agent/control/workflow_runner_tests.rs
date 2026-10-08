use super::*;

fn call(model: &str, effort: Option<&str>) -> wire::AgentCall {
    wire::AgentCall {
        name: "scouting:propose".into(),
        prompt: "Rechercher en francais".into(),
        role: None,
        phase: "Scouting".into(),
        schema: Value::Null,
        model: Some(model.into()),
        effort: effort.map(str::to_owned),
    }
}

#[test]
fn aliases_select_the_captured_native_model_and_explicit_effort_wins() {
    for (alias, effort) in [
        ("opus", ReasoningEffort::High),
        ("sonnet", ReasoningEffort::Medium),
        ("haiku", ReasoningEffort::Low),
    ] {
        let prepared = native_call(&call(alias, None), 1, 0, "gpt-6.1").unwrap();
        assert_eq!(prepared.model.as_deref(), Some("gpt-6.1"));
        assert_eq!(prepared.effort, Some(effort));
        assert_eq!(prepared.label.as_deref(), Some("scouting:propose"));
        assert_eq!(prepared.task_name, "wf_1_0");
        assert!(prepared.schema.is_null());
    }
    let prepared = native_call(&call("opus", Some("low")), 2, 3, "gpt-6.1").unwrap();
    assert_eq!(prepared.effort, Some(ReasoningEffort::Low));
    assert!(native_call(&call("opus", Some("fabricated")), 1, 0, "gpt-6.1").is_err());
}

#[test]
fn native_model_ids_are_preserved_for_native_catalog_validation() {
    let prepared = native_call(&call("gpt-6.1", None), 1, 1, "parent").unwrap();
    assert_eq!(prepared.model.as_deref(), Some("gpt-6.1"));
    assert!(prepared.compatible);
    assert_eq!(prepared.task_name, "wf_1_1");
}
