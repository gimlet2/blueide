//! Async LSP client that manages an arbitrary language server subprocess
//! and communicates with it over stdin/stdout using JSON-RPC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use super::protocol::{
    completion_request, definition_request, did_change_notification, did_close_notification,
    did_open_notification, encode_message, exit_notification, hover_request,
    initialize_request, initialized_notification, parse_content_length, shutdown_request,
    CompletionItem, Diagnostic, Location,
};

// ---------------------------------------------------------------------------
// Events emitted by the LSP client to the application
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LspEvent {
    /// Diagnostics published by the server for a URI.
    Diagnostics {
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    /// The server process exited unexpectedly.
    ServerExited,
}

// ---------------------------------------------------------------------------
// Internal messages from app→writer task
// ---------------------------------------------------------------------------

enum OutgoingMsg {
    Send(Vec<u8>),
    Shutdown,
}

// ---------------------------------------------------------------------------
// LspClient
// ---------------------------------------------------------------------------

/// Async LSP client.
///
/// Call [`LspClient::new`] with an explicit server command (built by
/// [`super::language::resolve_server`]) or `None` to run with no LSP.
/// Call [`LspClient::start`] to launch the language server and perform the
/// LSP handshake.  Once started, use the `complete`, `definition`, `hover`,
/// `open_file`, `change_file`, `close_file` methods.
/// Call [`LspClient::stop`] for a graceful shutdown.
pub struct LspClient {
    root_path: PathBuf,
    /// Full argv for the server process (argv[0] = executable).
    /// `None` → LSP is disabled (no binary found, Docker unavailable, etc.).
    server_cmd: Option<Vec<String>>,

    // Runtime state — set after `start()`.
    next_id: Arc<AtomicU64>,
    /// Pending request futures keyed by request id.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    /// Sender half for outgoing messages (to writer task).
    outgoing_tx: Option<mpsc::UnboundedSender<OutgoingMsg>>,
    /// Channel from which the app reads LSP events (diagnostics, etc.).
    pub events_rx: Option<mpsc::UnboundedReceiver<LspEvent>>,
    /// Open file version counters.
    open_versions: HashMap<String, i32>,

    initialized: bool,
}

impl LspClient {
    /// Create a new client rooted at `root_path`.
    ///
    /// `server_cmd` is the full argv for the language-server process:
    /// - `Some(argv)` — launch that command.
    /// - `None` — start without any server (LSP features will be silently
    ///   unavailable).
    ///
    /// The command is typically built by
    /// [`super::language::resolve_server`] and converted with
    /// [`super::language::ServerLaunch::into_argv`].
    pub fn new(root_path: PathBuf, server_cmd: Option<Vec<String>>) -> Self {
        Self {
            root_path,
            server_cmd,
            next_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            outgoing_tx: None,
            events_rx: None,
            open_versions: HashMap::new(),
            initialized: false,
        }
    }

    /// Returns `true` if the language server is running and initialized.
    pub fn is_available(&self) -> bool {
        self.initialized
    }

    // ------------------------------------------------------------------
    // Lifecycle
    // ------------------------------------------------------------------

    /// Start the language server and perform the LSP initialize handshake.
    ///
    /// Returns `Ok(true)` if the server was started and initialized
    /// successfully, `Ok(false)` if `server_cmd` is `None` (no server
    /// configured).
    pub async fn start(&mut self) -> Result<bool> {
        let cmd = match &self.server_cmd {
            Some(v) if !v.is_empty() => v.clone(),
            _ => {
                log::info!("no LSP server command configured; LSP disabled");
                return Ok(false);
            }
        };

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn {}", cmd[0]))?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;

        // Channel for outgoing messages (app → writer task).
        let (out_tx, out_rx) = mpsc::unbounded_channel::<OutgoingMsg>();
        // Channel for LSP events (reader task → app).
        let (events_tx, events_rx) = mpsc::unbounded_channel::<LspEvent>();

        let pending = Arc::clone(&self.pending);

        // Spawn writer task.
        tokio::spawn(writer_task(stdin, out_rx));

        // Spawn reader task.
        tokio::spawn(reader_task(stdout, pending, events_tx, child));

        self.outgoing_tx = Some(out_tx);
        self.events_rx = Some(events_rx);

        // Perform LSP initialize handshake.
        let root_uri = url_from_path(&self.root_path);
        let init_req = initialize_request(self.alloc_id(), &root_uri);
        self.send_request(init_req).await?;
        self.send_notification(initialized_notification()).await?;

        self.initialized = true;
        log::info!("LSP initialized at {root_uri}");
        Ok(true)
    }

    /// Gracefully shut down the language server.
    pub async fn stop(&mut self) {
        if !self.initialized {
            return;
        }
        let id = self.alloc_id();
        let _ = self.send_request(shutdown_request(id)).await;
        let _ = self.send_notification(exit_notification()).await;
        if let Some(tx) = self.outgoing_tx.take() {
            let _ = tx.send(OutgoingMsg::Shutdown);
        }
        self.initialized = false;
    }

    // ------------------------------------------------------------------
    // File operations
    // ------------------------------------------------------------------

    pub async fn open_file(&mut self, uri: &str, text: &str, language_id: &str) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }
        self.open_versions.insert(uri.to_string(), 1);
        self.send_notification(did_open_notification(uri, text, language_id)).await
    }

    pub async fn change_file(&mut self, uri: &str, text: &str) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }
        let version = self.open_versions.entry(uri.to_string()).or_insert(0);
        *version += 1;
        let v = *version;
        self.send_notification(did_change_notification(uri, text, v)).await
    }

    pub async fn close_file(&mut self, uri: &str) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }
        self.open_versions.remove(uri);
        self.send_notification(did_close_notification(uri)).await
    }

    // ------------------------------------------------------------------
    // LSP features
    // ------------------------------------------------------------------

    pub async fn complete(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<CompletionItem>> {
        if !self.initialized {
            return Ok(vec![]);
        }
        let id = self.alloc_id();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.send_request(completion_request(id, uri, line, character)),
        )
        .await
        .context("completion timeout")?
        .unwrap_or(Value::Null);

        let items: Vec<Value> = match result {
            Value::Array(arr) => arr,
            Value::Object(ref m) => m
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => vec![],
        };
        Ok(items.iter().filter_map(CompletionItem::from_value).collect())
    }

    pub async fn definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        if !self.initialized {
            return Ok(vec![]);
        }
        let id = self.alloc_id();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.send_request(definition_request(id, uri, line, character)),
        )
        .await
        .context("definition timeout")?
        .unwrap_or(Value::Null);

        let locs: Vec<Value> = match result {
            Value::Array(arr) => arr,
            Value::Object(_) => vec![result],
            _ => vec![],
        };
        Ok(locs
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect())
    }

    pub async fn hover(&mut self, uri: &str, line: u32, character: u32) -> Result<String> {
        if !self.initialized {
            return Ok(String::new());
        }
        let id = self.alloc_id();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.send_request(hover_request(id, uri, line, character)),
        )
        .await
        .context("hover timeout")?
        .unwrap_or(Value::Null);

        Ok(extract_hover_text(&result))
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn send_raw(&self, data: Vec<u8>) -> Result<()> {
        let tx = self.outgoing_tx.as_ref().context("LSP not started")?;
        tx.send(OutgoingMsg::Send(data))
            .map_err(|_| anyhow::anyhow!("LSP writer task closed"))?;
        Ok(())
    }

    async fn send_notification(&self, payload: Value) -> Result<()> {
        self.send_raw(encode_message(&payload)).await
    }

    async fn send_request(&self, payload: Value) -> Result<Value> {
        let id = payload["id"]
            .as_u64()
            .context("request has no id")?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.send_raw(encode_message(&payload)).await?;

        rx.await.map_err(|_| anyhow::anyhow!("LSP request {id} cancelled"))
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

/// Writes outgoing messages to the server's stdin.
async fn writer_task(mut stdin: ChildStdin, mut rx: mpsc::UnboundedReceiver<OutgoingMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            OutgoingMsg::Send(data) => {
                if stdin.write_all(&data).await.is_err() {
                    break;
                }
            }
            OutgoingMsg::Shutdown => break,
        }
    }
}

/// Reads incoming messages from the server's stdout, dispatches responses
/// to waiting futures and publishes notifications (diagnostics) as events.
async fn reader_task(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    events_tx: mpsc::UnboundedSender<LspEvent>,
    mut child: Child,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        // Read headers until \r\n\r\n.
        let header = match read_until_double_crlf(&mut reader).await {
            Ok(h) if !h.is_empty() => h,
            _ => break,
        };
        let content_length = match parse_content_length(&header) {
            Some(n) => n,
            None => continue,
        };

        // Read body.
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).await.is_err() {
            break;
        }

        let message: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        dispatch_message(message, &pending, &events_tx).await;
    }

    // Server exited — clean up pending futures and notify app.
    let mut map = pending.lock().await;
    map.clear();
    drop(map);
    let _ = events_tx.send(LspEvent::ServerExited);
    let _ = child.wait().await;
}

async fn dispatch_message(
    message: Value,
    pending: &Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    events_tx: &mpsc::UnboundedSender<LspEvent>,
) {
    // Response to a previous request.
    if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
        if message.get("method").is_none() {
            let mut map = pending.lock().await;
            if let Some(tx) = map.remove(&id) {
                let result = message["result"].clone();
                let _ = tx.send(result);
            }
            return;
        }
    }

    // Server-initiated notification.
    if let Some(method) = message.get("method").and_then(|v| v.as_str()) {
        if method == "textDocument/publishDiagnostics" {
            let params = &message["params"];
            let uri = params["uri"].as_str().unwrap_or("").to_string();
            let diagnostics = params["diagnostics"]
                .as_array()
                .map(|arr| arr.iter().filter_map(Diagnostic::from_value).collect())
                .unwrap_or_default();
            let _ = events_tx.send(LspEvent::Diagnostics { uri, diagnostics });
        }
    }
}

/// Read bytes from `reader` until the `\r\n\r\n` header terminator.
async fn read_until_double_crlf(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match reader.read_exact(&mut byte).await {
            Ok(_) => {}
            Err(_) => return Ok(buf),
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            return Ok(buf);
        }
    }
}

// ---------------------------------------------------------------------------
// Path → URI helper
// ---------------------------------------------------------------------------

pub fn url_from_path(path: &std::path::Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(path)
    };
    format!("file://{}", abs.to_string_lossy().replace('\\', "/"))
}

fn extract_hover_text(result: &Value) -> String {
    match result {
        Value::Null => String::new(),
        Value::Object(m) => {
            let contents = &m["contents"];
            match contents {
                Value::String(s) => s.clone(),
                Value::Object(c) => {
                    c.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string()
                }
                Value::Array(arr) => arr
                    .iter()
                    .map(|item| match item {
                        Value::String(s) => s.as_str(),
                        Value::Object(o) => o.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                        _ => "",
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

