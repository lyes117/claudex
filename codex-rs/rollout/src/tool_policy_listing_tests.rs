use super::*;
use codex_protocol::protocol::SessionMeta;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn listing_readers_reject_invalid_canonical_headers_before_valid_ancestors() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    let ancestor = json!({"timestamp":"2026-10-03T00:00:00Z","type":"session_meta",
        "payload":SessionMeta::default()});
    let id: ThreadId = serde_json::from_value(ancestor["payload"]["id"].clone()).unwrap();
    tokio::fs::write(&path, format!("{ancestor}\n"))
        .await
        .unwrap();
    assert_eq!(
        read_head_summary(&path, HEAD_RECORD_LIMIT)
            .await
            .unwrap()
            .thread_id,
        Some(id)
    );
    assert_eq!(read_head_for_summary(&path).await.unwrap().len(), 1);
    for kind in [json!("session_meta"), json!(null), json!("event_msg")] {
        let mut canonical = ancestor.clone();
        canonical["type"] = kind;
        canonical["payload"]["tool_policy_snapshot"] = json!({"version":2});
        tokio::fs::write(&path, format!("{canonical}\n{ancestor}\n"))
            .await
            .unwrap();
        assert_eq!(
            read_head_summary(&path, HEAD_RECORD_LIMIT)
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            read_head_for_summary(&path).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
