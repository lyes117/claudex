use std::marker::PhantomData;
use std::sync::Arc;

use codex_exec_server::LocalFileSystem;
use codex_file_system::FileSystemSandboxContext;
use codex_protocol::models::PermissionProfile;
use codex_tools::ToolEnvironment;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

pub(super) fn environment(path: &std::path::Path) -> ToolEnvironment<'static> {
    let cwd = PathUri::from_host_native_path(path).unwrap();
    ToolEnvironment {
        environment_id: "fixture".into(),
        cwd: cwd.clone(),
        file_system: Arc::new(LocalFileSystem::unsandboxed()),
        file_system_sandbox_context: FileSystemSandboxContext::from_permission_profile(
            PermissionProfile::Disabled,
            cwd,
        ),
        _lifetime: PhantomData,
    }
}

#[tokio::test]
async fn truncated_walk_still_searches_all_collected_files_and_forwards_policy() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.txt"), "nothing\n").unwrap();
    std::fs::write(directory.path().join("b.txt"), "needle\n").unwrap();
    let mut environment = environment(directory.path());
    let mut file_system =
        test_file_system::CheckedFileSystem::new(environment.file_system_sandbox_context.clone());
    file_system.walk_truncated = true;
    environment.file_system = Arc::new(file_system);
    assert_eq!(
        search::search(
            &environment,
            r#"{"pattern":"needle","output_mode":"content"}"#,
            MAX_RESPONSE_BYTES,
            true
        )
        .await
        .unwrap(),
        json!({"results":[{"path":"b.txt","line":1,"text":"needle"}],"skipped":0,"truncated":true})
    );
    assert_eq!(
        read::read(&environment, r#"{"file_path":"b.txt"}"#, MAX_RESPONSE_BYTES)
            .await
            .unwrap()["lines"],
        json!([{"line":1,"text":"needle"}])
    );
}

#[tokio::test]
async fn read_offsets_utf8_and_crlf_use_real_files() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("text.md"),
        "premier\r\nété 🦊\r\ntroisième\n",
    )
    .unwrap();
    let value = read::read(
        &environment(directory.path()),
        r#"{"file_path":"text.md","offset":2,"limit":1}"#,
        MAX_RESPONSE_BYTES,
    )
    .await
    .unwrap();
    assert_eq!(
        value,
        json!({"lines":[{"line":2,"text":"été 🦊"}],"offset":2,"truncated":true})
    );
}

#[tokio::test]
async fn read_rejects_binary_large_and_invalid_arguments() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("binary"), [0, 1, 2]).unwrap();
    std::fs::write(directory.path().join("invalid"), [255]).unwrap();
    std::fs::write(
        directory.path().join("large"),
        vec![b'a'; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    let environment = environment(directory.path());
    for arguments in [
        r#"{"file_path":"binary"}"#,
        r#"{"file_path":"invalid"}"#,
        r#"{"file_path":"large"}"#,
        r#"{"file_path":"binary","offset":0}"#,
        r#"{"file_path":"binary","pages":"1"}"#,
    ] {
        assert!(
            read::read(&environment, arguments, MAX_RESPONSE_BYTES)
                .await
                .is_err(),
            "{arguments}"
        );
    }
}

#[tokio::test]
async fn glob_and_grep_search_real_files_with_filters_modes_and_offsets() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    std::fs::write(directory.path().join("src/a.rs"), "Alpha\nalpha\nomega\n").unwrap();
    std::fs::write(directory.path().join("src/b.rs"), "alpha\n").unwrap();
    std::fs::write(directory.path().join("other.txt"), "alpha\n").unwrap();
    let environment = environment(directory.path());
    assert_eq!(
        search::search(
            &environment,
            r#"{"pattern":"**/*.rs"}"#,
            MAX_RESPONSE_BYTES,
            false
        )
        .await
        .unwrap(),
        json!({"results":["src/a.rs","src/b.rs"],"skipped":0,"truncated":false})
    );
    assert_eq!(
        search::search(
            &environment,
            r#"{"pattern":"alpha","glob":"**/*.rs","-i":true,"output_mode":"count"}"#,
            MAX_RESPONSE_BYTES,
            true
        )
        .await
        .unwrap(),
        json!({"results":[{"path":"src/a.rs","count":2},{"path":"src/b.rs","count":1}],"skipped":0,"truncated":false})
    );
    assert_eq!(
        search::search(
            &environment,
            r#"{"pattern":"alpha","glob":"**/*.rs","output_mode":"content","offset":1}"#,
            MAX_RESPONSE_BYTES,
            true
        )
        .await
        .unwrap(),
        json!({"results":[{"path":"src/b.rs","line":1,"text":"alpha"}],"skipped":0,"truncated":false})
    );
    assert!(
        search::search(&environment, r#"{"pattern":"["}"#, MAX_RESPONSE_BYTES, true)
            .await
            .is_err()
    );
    assert!(
        search::search(
            &environment,
            r#"{"pattern":"x","multiline":true}"#,
            MAX_RESPONSE_BYTES,
            true
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn grep_counts_skipped_binary_and_limits_matches() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.bin"), [0]).unwrap();
    std::fs::write(directory.path().join("b.txt"), "x\nx\nx\n").unwrap();
    let environment = environment(directory.path());
    assert_eq!(
        search::search(
            &environment,
            r#"{"pattern":"x","output_mode":"content","head_limit":1}"#,
            MAX_RESPONSE_BYTES,
            true
        )
        .await
        .unwrap(),
        json!({"results":[{"path":"b.txt","line":1,"text":"x"}],"skipped":1,"truncated":true})
    );
}

#[tokio::test]
async fn response_budget_accounts_for_json_escaping() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("text"), "\\\"\t\u{1f}".repeat(2000)).unwrap();
    let result = read::read(
        &environment(directory.path()),
        r#"{"file_path":"text"}"#,
        600,
    )
    .await
    .unwrap();
    assert!(result.to_string().len() <= 600);
    assert_eq!(result["truncated"], true);
}

#[tokio::test]
async fn direct_and_code_mode_outputs_bound_adversarial_token_dense_text() {
    use codex_protocol::protocol::TruncationPolicy;
    use codex_tools::ConversationHistory;
    use codex_tools::NoopTurnItemEmitter;
    use codex_tools::ToolCallSource;
    use codex_tools::ToolPayload;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("dense"),
        "j~%#;$^&*[]!\n".repeat(1500),
    )
    .unwrap();
    for source in [
        ToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id: "cell".into(),
            runtime_tool_call_id: "nested".into(),
        },
    ] {
        let call = ToolCall {
            turn_id: "turn".into(),
            call_id: "read".into(),
            tool_name: ToolName::plain("Read"),
            model: "fixture".into(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(MAX_RESPONSE_BYTES),
            source,
            conversation_history: ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: vec![environment(directory.path())],
            payload: ToolPayload::Function {
                arguments: r#"{"file_path":"dense"}"#.into(),
            },
        };
        let publications = Arc::new(publication::Publications::default());
        publications.admit(
            &Arc::new(publication::TurnCalls::default()),
            &call.turn_id,
            &call.call_id,
        );
        let turn_id = call.turn_id.clone();
        let call_id = call.call_id.clone();
        let result = NativeFileTool {
            tool: FileTool::Read,
            publications: publications.clone(),
        }
        .handle(call)
        .await
        .unwrap()
        .log_output();
        publications
            .finish(
                &turn_id,
                &call_id,
                codex_extension_api::ToolCallOutcome::Completed { success: true },
                codex_extension_api::ToolResultDisposition::Unchanged,
            )
            .await;
        assert!(result.len() <= 8192);
        assert!(
            result.len() > 6000,
            "The bound must be exercised by a nonempty response"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap()["truncated"],
            true
        );
    }
}

#[tokio::test]
async fn managed_read_restrictions_fail_closed_without_a_sandbox_backend() {
    use codex_protocol::models::SandboxEnforcement;
    use codex_protocol::permissions::FileSystemSandboxPolicy;
    use codex_protocol::permissions::NetworkSandboxPolicy;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("secret-fixture"), "SYNTHETIC_CONTENT").unwrap();
    let mut environment = environment(directory.path());
    environment.file_system_sandbox_context.permissions =
        PermissionProfile::from_runtime_permissions_with_enforcement(
            SandboxEnforcement::Managed,
            &FileSystemSandboxPolicy::restricted(Vec::new()),
            NetworkSandboxPolicy::Restricted,
        );
    assert!(
        validate_environment(&environment).is_err()
            || read::read(
                &environment,
                r#"{"file_path":"secret-fixture"}"#,
                MAX_RESPONSE_BYTES
            )
            .await
            .is_err()
    );
}
