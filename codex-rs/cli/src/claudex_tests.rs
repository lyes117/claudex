//! Tests for the claudex local utilities (zcode routing helpers).

use serde_json::json;

use crate::claudex::texte_parcouru;

#[test]
fn claudex_zcode_texte_parcouru_reads_the_content_field_of_the_result() {
    assert_eq!(
        texte_parcouru(&json!({"content": "bonjour"})),
        Some("bonjour".into())
    );
    assert_eq!(
        texte_parcouru(&json!({"text": "salut"})),
        Some("salut".into())
    );
}

#[test]
fn claudex_zcode_texte_parcouru_reads_the_accepted_ack_without_inventing_text() {
    // Measured session/send reply: no reply-ish key — the caller must treat the
    // turn as textless and rely on the terminal event instead.
    assert_eq!(
        texte_parcouru(&json!({"accepted": true, "sessionId": "sess_x"})),
        None
    );
}

#[test]
fn claudex_zcode_texte_parcouru_prefers_reply_keys_then_digs_deeper() {
    assert_eq!(
        texte_parcouru(&json!({"deep": {"text": "trouvé"}})),
        Some("trouvé".into())
    );
    assert_eq!(
        texte_parcouru(&json!([{"content": "liste"}])),
        Some("liste".into())
    );
    assert_eq!(texte_parcouru(&json!({"number": 3})), None);
}
