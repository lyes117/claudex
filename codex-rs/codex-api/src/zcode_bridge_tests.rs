//! Tests for the ZCode bridge: framing/envelope in pure Rust (deterministic, no
//! child process) plus one `#[ignore]` integration test replaying the measured
//! fixture (initialize + session/create, zero inference).

use std::io;
use std::path::Path;

use serde_json::json;

use crate::zcode_bridge::CLI_PATH_ENV;
use crate::zcode_bridge::DEFAULT_CLI_PATH;
use crate::zcode_bridge::Incoming;
use crate::zcode_bridge::Outcome;
use crate::zcode_bridge::SessionCreated;
use crate::zcode_bridge::TurnTerminal;
use crate::zcode_bridge::ZcodeBridge;
use crate::zcode_bridge::answer_server_request;
use crate::zcode_bridge::classify;
use crate::zcode_bridge::encode_line;
use crate::zcode_bridge::parse_line;
use crate::zcode_bridge::session_created_from;
use crate::zcode_bridge::terminal_from;

// —— Framing (measured: `JSON.stringify(msg) + "\n"`, no `jsonrpc` field) ——

#[test]
fn encode_line_is_compact_single_line_without_jsonrpc() {
    let line =
        encode_line(&json!({"method": "session/create", "id": 2, "params": {"workspace": {}}}));
    // Key order follows serde_json feature unification (preserve_order can come
    // from any package built alongside this one) — assert the properties that
    // matter, not an exact serialization.
    assert_eq!(line.matches('\n').count(), 1);
    assert!(!line.contains("jsonrpc"));
    let parsed: serde_json::Value =
        serde_json::from_str(line.trim()).expect("the line must be one JSON object");
    assert_eq!(parsed["id"], 2);
    assert_eq!(parsed["method"], "session/create");
}

#[test]
fn parse_line_skips_blank_and_non_json_lines() {
    assert_eq!(parse_line(""), None);
    assert_eq!(parse_line("   \r\n"), None);
    assert_eq!(parse_line("not json at all"), None);
    assert_eq!(parse_line("  {\"a\":1}  "), Some(json!({"a": 1})));
}

#[test]
fn framing_round_trips_through_one_line() {
    let msg = json!({"method": "initialize", "id": 1, "params": {"clientInfo": {"name": "probe"}}});
    assert_eq!(parse_line(&encode_line(&msg)), Some(msg));
}

// —— Envelope classification (measured shapes) ——

#[test]
fn classify_recognizes_the_three_measured_shapes() {
    // Server→client request: id is the STRING "server-<n>" (measured fact).
    assert_eq!(
        classify(&json!({
            "id": "server-1",
            "method": "session/requestRuntimePreferences",
            "params": {"sessionId": "sess_x", "scope": "runtime-materialization"}
        })),
        Some(Incoming::Request {
            id: json!("server-1"),
            method: "session/requestRuntimePreferences".into(),
            params: Some(json!({"sessionId": "sess_x", "scope": "runtime-materialization"})),
        })
    );
    // Notification: {method} without id.
    assert_eq!(
        classify(
            &json!({"method": "startup/storageState", "params": {"phase": "ready", "schemaVersion": 1}})
        ),
        Some(Incoming::Notification {
            method: "startup/storageState".into(),
            params: Some(json!({"phase": "ready", "schemaVersion": 1})),
        })
    );
    // Response with result.
    assert_eq!(
        classify(&json!({"id": 2, "result": {"protocol": {"version": 1}}})),
        Some(Incoming::Response(Outcome::Ok(
            json!({"protocol": {"version": 1}})
        )))
    );
    // Response with the measured -32601 error for initialize.
    assert_eq!(
        classify(
            &json!({"id": 1, "error": {"code": -32601, "message": "Method not found: initialize"}})
        ),
        Some(Incoming::Response(Outcome::Err {
            code: -32601,
            message: "Method not found: initialize".into(),
            data: None,
        }))
    );
    // Off-envelope: ignored.
    assert_eq!(classify(&json!({})), None);
    assert_eq!(classify(&json!([1, 2])), None);
    assert_eq!(classify(&json!(3)), None);
}

// —— Server→client replies (measured: without one, session/create fails -32022) ——

#[test]
fn server_requests_get_the_measured_replies() {
    // The runtime preference: the pGt schema minimum, exactly as the fixture answered.
    assert_eq!(
        answer_server_request(&json!("server-1"), "session/requestRuntimePreferences"),
        json!({"id": "server-1", "result": {"nativeSearchEnhancementsEnabled": false}})
    );
    // Any other method: a -32601 error, which the server tolerates (measured).
    assert_eq!(
        answer_server_request(
            &json!("server-2"),
            "interaction/requestOfficialMcpAuthHeaders"
        ),
        json!({"id": "server-2", "error": {"code": -32601, "message": "zcode-bridge: server method not supported"}})
    );
}

// —— session/create result parsing (measured snapshot) ——

#[test]
fn session_create_reads_the_measured_snapshot() {
    let result = json!({
        "messages": [],
        "projection": {"contextWindow": 200000, "contextUsed": 0, "mode": "build", "status": "idle", "turnCount": 0},
        "protocol": {"name": "ZCode Protocol", "version": 1},
        "session": {
            "sessionId": "sess_43a9c670-4dbb-4bac-ad2b-7c64841bdc1d",
            "sessionKind": "interactive",
            "status": "idle",
            "mode": "build",
            "title": ""
        },
        "settings": {"mode": {"current": "build"}, "model": {"available": []}},
        "todos": [],
        "todoGroups": []
    });
    assert_eq!(
        session_created_from(&result),
        SessionCreated {
            session_id: "sess_43a9c670-4dbb-4bac-ad2b-7c64841bdc1d".into(),
            mode: "build".into(),
            protocol_name: "ZCode Protocol".into(),
            protocol_version: 1,
        }
    );
}

// —— Fixture replay (real child, zero inference) ——

#[test]
#[ignore = "spawns the real ZCode CLI — replays the measured fixture (initialize + session/create, 0 inference)"]
fn replays_the_real_protocol_fixture() -> io::Result<()> {
    if std::env::var_os(CLI_PATH_ENV).is_none() && !Path::new(DEFAULT_CLI_PATH).exists() {
        eprintln!("ZCode CLI not found ({DEFAULT_CLI_PATH}) — fixture not replayed");
        return Ok(());
    }
    let workspace = tempfile::tempdir()?;
    let mut bridge = ZcodeBridge::spawn()?;

    // Measured: initialize is refused -32601 (no handshake exists).
    match bridge.initialize()? {
        Outcome::Err { code, message, .. } => {
            assert_eq!(code, -32601);
            assert!(
                message.contains("initialize"),
                "unexpected message: {message}"
            );
        }
        other => panic!("initialize should be refused with -32601, got {other:?}"),
    }

    // Measured: the {workspace} form succeeds once the two server→client requests
    // are answered (the bridge answers them); the {cwd} form is refused -32602.
    let path = workspace
        .path()
        .to_str()
        .ok_or_else(|| io::Error::other("workspace path is not utf-8"))?;
    let created = bridge.session_create(path)?;
    assert!(
        created.session_id.starts_with("sess_"),
        "unexpected session id: {}",
        created.session_id
    );
    assert_eq!(created.protocol_name, "ZCode Protocol");
    assert_eq!(created.protocol_version, 1);
    assert_eq!(created.mode, "build");

    // Measured clean close: stdin.end() → exit code 0, kill never needed.
    let exit = bridge.close()?;
    assert_eq!(exit, 0, "the child must exit cleanly after stdin is closed");
    Ok(())
}

// —— Turn terminal (frames captured on the first REAL turn, see
//    protocol-turn-probe.log: that turn failed on model selection, which is how
//    the failure shape got measured) ——

#[test]
fn terminal_from_reads_the_measured_telemetry_terminal_event() {
    let frame = json!({
        "method": "v4/telemetry/event",
        "params": {
            "kind": "turn.terminal",
            "status": "failed",
            "errorCode": "CONFIGURATION_ERROR",
            "errorMessage": "Select a model before continuing",
        }
    });
    let terminal = terminal_from(&frame).expect("turn.terminal must be detected");
    assert_eq!(
        terminal,
        TurnTerminal {
            status: "failed".to_owned(),
            error_message: "Select a model before continuing".to_owned(),
        }
    );
}

#[test]
fn terminal_from_reads_the_measured_operation_turn_failed_event() {
    let frame = json!({
        "method": "computer-use/operation-event",
        "params": {"kind": "turn-failed", "turnId": "turn_x"}
    });
    let terminal = terminal_from(&frame).expect("turn-failed must be detected");
    // Normalized: the caller branches on one vocabulary (failed/completed).
    assert_eq!(terminal.status, "failed");
    assert_eq!(terminal.error_message, "");
}

#[test]
fn terminal_from_ignores_non_terminal_events() {
    let accepted = json!({"id": 2, "result": {"accepted": true, "sessionId": "sess_x"}});
    assert_eq!(terminal_from(&accepted), None);
    let unrelated = json!({"method": "startup/storageState", "params": {"kind": "ready"}});
    assert_eq!(terminal_from(&unrelated), None);
    let streaming = json!({"method": "session/event", "params": {"kind": "item/delta"}});
    assert_eq!(terminal_from(&streaming), None);
}
