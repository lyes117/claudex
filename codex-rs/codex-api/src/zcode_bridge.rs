//! Minimal bridge to the ZCode CLI (`zcode.cjs app-server --stdio`).
//!
//! Everything here is measured against the real protocol — facts and raw fixture:
//! `.build-tools/zai-continuation/bundle-analysis/protocol-facts.json`
//! (fixture: `protocol-fixture.mjs`, 2 runs, clean exit 0, zero inference):
//! - Framing: NDJSON — one request per line, `JSON.stringify(msg) + "\n"` on stdin
//!   (`ZCodeProtocolNdjsonConnection` in the bundle); replies on stdout, one per
//!   line, blank and non-JSON lines ignored. NO `jsonrpc` field anywhere.
//! - No handshake: `initialize` does not exist server-side (it replies -32601).
//! - Envelope: request `{method,id,params}` · response `{id,result|error}` ·
//!   notification `{method}` without id. Client ids are numbers, server→client
//!   request ids are strings `"server-<n>"`.
//! - `session/create` requires `{"workspace":{"workspacePath","workspaceKey"}}`
//!   (strict schema; the `{"cwd":…}` form is refused -32602) and only succeeds if
//!   the client answers the server→client requests (otherwise -32022 after 15 s).

use std::io::BufRead;
use std::io::Write;
use std::io::{self};
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;

/// Default CLI path, measured on this machine by the protocol fixture
/// (override with the `ZCODE_CLI_PATH` environment variable).
pub const DEFAULT_CLI_PATH: &str =
    "C:/Users/lyesb/AppData/Local/Programs/ZCode/resources/glm/zcode.cjs";
pub(crate) const CLI_PATH_ENV: &str = "ZCODE_CLI_PATH";

/// Close grace period: the measured server exits 32–54 ms after `stdin.end()`.
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// Grace after a first message-less terminal event: the measured telemetry
/// terminal carrying `errorMessage` follows 9 ms later.
const TERMINAL_GRACE: Duration = Duration::from_secs(2);

/// Measured framing: `JSON.stringify(msg) + "\n"` — compact, one line per message.
pub fn encode_line(msg: &Value) -> String {
    format!("{msg}\n")
}

/// Measured decoding: blank lines skipped, non-JSON lines ignored (the server does
/// the same with ours: `zcode_protocol.parse.failed` event, no disconnection).
pub fn parse_line(line: &str) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// A server reply to one of our requests — measured envelope `{id, result|error}`,
/// JSON-RPC-style numbering but WITHOUT a `jsonrpc` field.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ok(Value),
    Err {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

/// An incoming message classified per the measured envelope.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Server→client request (string id `"server-<n>"`): the server awaits a reply.
    Request {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    Response(Outcome),
    Notification {
        method: String,
        params: Option<Value>,
    },
}

/// Classify a message per the measured envelope; `None` = not on the wire shape (ignored).
pub fn classify(msg: &Value) -> Option<Incoming> {
    let obj = msg.as_object()?;
    let has_method = obj.contains_key("method");
    let has_id = obj.contains_key("id");
    if has_method && has_id {
        return Some(Incoming::Request {
            id: obj["id"].clone(),
            method: obj["method"].as_str()?.to_owned(),
            params: obj.get("params").cloned(),
        });
    }
    if has_id {
        if let Some(err) = obj.get("error") {
            return Some(Incoming::Response(Outcome::Err {
                code: err.get("code").and_then(Value::as_i64)?,
                message: err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                data: err.get("data").cloned(),
            }));
        }
        return Some(Incoming::Response(Outcome::Ok(
            obj.get("result").cloned().unwrap_or(Value::Null),
        )));
    }
    if has_method {
        return Some(Incoming::Notification {
            method: obj["method"].as_str()?.to_owned(),
            params: obj.get("params").cloned(),
        });
    }
    None
}

/// A turn's terminal state, extracted from the events measured on a real turn
/// (`v4/telemetry/event` kind `turn.terminal` with `status`/`errorMessage`,
/// `computer-use/operation-event` kind `turn-failed`; the completed-side kinds
/// are the symmetric guesses — calibrate on the first successful turn).
#[derive(Debug, Clone, PartialEq)]
pub struct TurnTerminal {
    pub status: String,
    pub error_message: String,
}

/// Extract terminal info from one incoming message. Pure, testable without a child.
/// The measured operation-event kind `turn-failed` is normalized to status `failed`
/// (and `turn-completed` to `completed`) so callers branch on one vocabulary.
pub fn terminal_from(msg: &Value) -> Option<TurnTerminal> {
    let params = msg.get("params")?;
    let kind = params.get("kind")?.as_str()?;
    let status = match kind {
        "turn.terminal" => params
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(kind)
            .to_owned(),
        "turn-failed" => "failed".to_owned(),
        "turn-completed" => "completed".to_owned(),
        _ => return None,
    };
    Some(TurnTerminal {
        status,
        error_message: params
            .get("errorMessage")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

/// Reply to a server→client request. Measured facts:
/// - without a reply, `session/create` fails after 15 s (-32022);
/// - `session/requestRuntimePreferences` expects the pGt schema where every field
///   but `nativeSearchEnhancementsEnabled` has a default — the fixture answers the minimum;
/// - any other method: the server TOLERATES a client-side -32601 error
///   (proven on `interaction/requestOfficialMcpAuthHeaders`).
///
/// ponytail: one runtime preference hardcoded; add the others only if a real run demands them.
pub(crate) fn answer_server_request(id: &Value, method: &str) -> Value {
    if method == "session/requestRuntimePreferences" {
        serde_json::json!({ "id": id, "result": { "nativeSearchEnhancementsEnabled": false } })
    } else {
        serde_json::json!({ "id": id, "error": { "code": -32601, "message": "zcode-bridge: server method not supported" } })
    }
}

/// The useful fields of the `session/create` result (measured snapshot,
/// protocol-facts.json §sessionCreate).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCreated {
    pub session_id: String,
    pub mode: String,
    pub protocol_name: String,
    pub protocol_version: u32,
}

pub(crate) fn session_created_from(result: &Value) -> SessionCreated {
    let str_at = |pointer: &str| {
        result
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    SessionCreated {
        session_id: str_at("/session/sessionId"),
        mode: str_at("/session/mode"),
        protocol_name: str_at("/protocol/name"),
        protocol_version: result
            .pointer("/protocol/version")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32,
    }
}

/// The result of one exchange: the reply to OUR id, plus the notifications
/// received while waiting (e.g. unsolicited `startup/storageState` pushes).
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub outcome: Outcome,
    pub notifications: Vec<Value>,
}

/// The bridge: one `node <cli> app-server --stdio` child and its NDJSON channel.
pub struct ZcodeBridge {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    reader: Option<JoinHandle<()>>,
    next_id: u64,
}

impl ZcodeBridge {
    /// Spawn `node <cli> app-server --stdio` (exact argv of the measured fixture).
    /// CLI path: `ZCODE_CLI_PATH` env var, else [`DEFAULT_CLI_PATH`].
    pub fn spawn() -> io::Result<Self> {
        let cli = std::env::var(CLI_PATH_ENV).unwrap_or_else(|_| DEFAULT_CLI_PATH.to_owned());
        let mut cmd = Command::new("node");
        cmd.arg(cli)
            .arg("app-server")
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()); // ponytail: stderr dropped — wire it when a run needs it
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // ponytail: CREATE_NO_WINDOW — the fixture's windowsHide:true, else a console flashes.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing child stdout"))?;
        let (tx, lines) = channel();
        // ponytail: reader on a thread — std has no read-with-deadline; the channel gives
        // bounded recv_timeout. Upgrade to tokio::process only if this bridge goes async.
        let reader = std::thread::spawn(move || {
            for line in io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break }; // EOF / read error: reader done
                if tx.send(line).is_err() {
                    break; // bridge dropped
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            lines,
            reader: Some(reader),
            next_id: 1,
        })
    }

    /// Send the request and read until the reply to OUR id (client ids are numbers,
    /// server ids are strings — different shapes cannot collide).
    fn exchange(&mut self, method: &str, params: Value, timeout: Duration) -> io::Result<Exchange> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({ "method": method, "id": id, "params": params });
        self.write_line(&encode_line(&request))?;
        let deadline = Instant::now() + timeout;
        let mut notifications = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::other(format!(
                        "zcode: no reply to {method} within {timeout:?}"
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("zcode: stdout closed — the child exited"));
                }
            };
            let Some(msg) = parse_line(&line) else {
                continue;
            };
            match classify(&msg) {
                Some(Incoming::Response(outcome))
                    if msg.get("id").and_then(Value::as_u64) == Some(id) =>
                {
                    return Ok(Exchange {
                        outcome,
                        notifications,
                    });
                }
                Some(Incoming::Request {
                    id: server_id,
                    method: server_method,
                    ..
                }) => {
                    let reply = answer_server_request(&server_id, &server_method);
                    self.write_line(&encode_line(&reply))?;
                }
                Some(Incoming::Notification { .. }) => notifications.push(msg),
                Some(Incoming::Response(_)) | None => {} // stale reply or off-envelope: ignored
            }
        }
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("zcode: stdin already closed"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.flush()
    }

    /// Measured fact: `initialize` DOES NOT EXIST server-side — no handshake is
    /// required, the expected reply is the -32601 error. Doubles as a liveness probe.
    pub fn initialize(&mut self) -> io::Result<Outcome> {
        let params = serde_json::json!({
            "clientInfo": { "name": "codex-bridge", "version": env!("CARGO_PKG_VERSION") }
        });
        Ok(self
            .exchange("initialize", params, Duration::from_secs(10))?
            .outcome)
    }

    /// `session/create` — measured form `{"workspace":{"workspacePath","workspaceKey"}}`
    /// (the `{"cwd":…}` form is refused -32602). Answers server→client requests along the way.
    pub fn session_create(&mut self, workspace_path: &str) -> io::Result<SessionCreated> {
        let params = serde_json::json!({
            "workspace": { "workspacePath": workspace_path, "workspaceKey": workspace_path }
        });
        match self
            .exchange("session/create", params, Duration::from_secs(30))?
            .outcome
        {
            Outcome::Ok(result) => Ok(session_created_from(&result)),
            Outcome::Err { code, message, .. } => Err(io::Error::other(format!(
                "session/create: {code} {message}"
            ))),
        }
    }

    /// Send one turn's text and collect the notifications until the reply to our id
    /// arrives. Params per the bundle's `EB` builder (`sessionId`, `content`,
    /// optional `modelSelection` — deliberately omitted: the account default model
    /// is used, no setModel). ponytail: the REPLY and turn-end shapes are NOT
    /// measured — the fixture never called session/send (zero inference);
    /// calibrate on a first real run before any actual use. Incremental streaming
    /// (delta by delta) is likewise unmeasured: this returns once the reply to our
    /// id arrives.
    pub fn session_send(&mut self, session_id: &str, text: &str) -> io::Result<Exchange> {
        let params = serde_json::json!({ "sessionId": session_id, "content": text });
        self.exchange("session/send", params, Duration::from_secs(120))
    }

    /// Drain events after the `session/send` reply until the turn reaches a
    /// terminal state (measured: `v4/telemetry/event` kind `turn.terminal` carrying
    /// `status`/`errorMessage`, and `computer-use/operation-event` kind
    /// `turn-failed`). The measured order on a failed turn is the message-less
    /// `turn-failed` first, then the telemetry terminal 9 ms later carrying the
    /// cause — so once a first terminal is seen the drain keeps a short grace to
    /// prefer one carrying `errorMessage`. Returns the notifications seen on the
    /// way plus the terminal state, or `None` on timeout. Answers server→client
    /// requests along the way.
    pub fn wait_terminal(
        &mut self,
        timeout: Duration,
    ) -> io::Result<(Vec<Value>, Option<TurnTerminal>)> {
        let deadline = Instant::now() + timeout;
        let mut first: Option<TurnTerminal> = None;
        let mut notifications = Vec::new();
        loop {
            let mut remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok((notifications, first));
            }
            if first.is_some() {
                // Short grace only: the message-bearing terminal follows in ms.
                remaining = remaining.min(TERMINAL_GRACE);
            }
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Ok((notifications, first.take()));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("zcode: stdout closed — the child exited"));
                }
            };
            let Some(msg) = parse_line(&line) else {
                continue;
            };
            if let Some(terminal) = terminal_from(&msg) {
                if !terminal.error_message.is_empty() {
                    return Ok((notifications, Some(terminal)));
                }
                if first.is_none() {
                    first = Some(terminal);
                }
                continue;
            }
            match classify(&msg) {
                Some(Incoming::Request {
                    id: server_id,
                    method: server_method,
                    ..
                }) => {
                    let reply = answer_server_request(&server_id, &server_method);
                    self.write_line(&encode_line(&reply))?;
                }
                Some(Incoming::Notification { .. }) => notifications.push(msg),
                _ => {}
            }
        }
    }

    /// Measured clean close: close stdin (the fixture's `stdin.end()`) → the server
    /// drains and exits with code 0; 5 s grace then kill as a last resort (never
    /// needed in the runs). Returns the child's exit code.
    pub fn close(&mut self) -> io::Result<i32> {
        drop(self.stdin.take());
        let deadline = Instant::now() + CLOSE_GRACE;
        let exited = loop {
            match self.child.try_wait()? {
                Some(status) => break Some(status),
                None if Instant::now() >= deadline => break None,
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        };
        let status = match exited {
            Some(status) => status,
            None => {
                self.child.kill()?;
                self.child.wait()?
            }
        };
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        Ok(status.code().unwrap_or(-1))
    }
}

impl Drop for ZcodeBridge {
    fn drop(&mut self) {
        self.stdin = None; // close stdin: the server exits by itself
        let _ = self.child.kill(); // safety net on our own child; no-op once exited
        let _ = self.child.wait();
    }
}
