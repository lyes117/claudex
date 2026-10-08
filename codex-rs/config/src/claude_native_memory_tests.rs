use super::*;
use pretty_assertions::assert_eq;
use std::fs;

#[test]
fn activation_is_bounded_strict_and_explicit() {
    let home = tempfile::tempdir().unwrap();
    assert!(!active_for_user(home.path()).unwrap());
    let directory = home.path().join(".claudex/memory");
    fs::create_dir_all(&directory).unwrap();
    let marker = directory.join("native-active.json");
    for active in [false, true] {
        let activation =
            serde_json::json!({"version":1,"active":active,"pluginId":"claude-mem@claudex-memory"});
        fs::write(&marker, activation.to_string()).unwrap();
        assert_eq!(active_for_user(home.path()).unwrap(), active);
    }
    for wire in [
        r#"{"version":1,"active":true,"pluginId":"other"}"#.to_owned(),
        r#"{"version":2,"active":true,"pluginId":"claude-mem@claudex-memory"}"#.to_owned(),
        r#"{"version":1,"active":"true","pluginId":"claude-mem@claudex-memory"}"#.to_owned(),
        r#"{"version":1,"active":true,"pluginId":"claude-mem@claudex-memory","extra":0}"#
            .to_owned(),
        r#"{"version":1,"active":false,"active":true,"pluginId":"claude-mem@claudex-memory"}"#
            .to_owned(),
        " ".repeat(MAX_ACTIVATION_BYTES as usize + 1),
    ] {
        fs::write(&marker, wire).unwrap();
        assert_eq!(
            active_for_user(home.path()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
