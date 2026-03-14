//! LSP (Language Server Protocol) JSON-RPC message types and builders.
//!
//! Implements the subset of LSP 3.17 used by BlueIDE:
//! initialize, textDocument/didOpen, textDocument/didChange,
//! textDocument/completion, textDocument/publishDiagnostics,
//! textDocument/hover, textDocument/definition, shutdown, exit.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// JSON-RPC wire format helpers
// ---------------------------------------------------------------------------

/// Build a JSON-RPC 2.0 request object.
pub fn make_request(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

/// Build a JSON-RPC 2.0 notification object (no `id`).
pub fn make_notification(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

/// Encode a JSON-RPC payload with an LSP `Content-Length` header.
pub fn encode_message(payload: &Value) -> Vec<u8> {
    let body = serde_json::to_string(payload).unwrap_or_default();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = header.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Parse the `Content-Length` value from a raw header byte slice.
pub fn parse_content_length(header: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(header).ok()?;
    for line in text.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("content-length:") {
            return line.splitn(2, ':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Common LSP request / notification builders
// ---------------------------------------------------------------------------

pub fn initialize_request(id: u64, root_uri: &str) -> Value {
    make_request(
        id,
        "initialize",
        json!({
            "processId": null,
            "clientInfo": { "name": "blueide", "version": "0.1.0" },
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionItem": {
                            "snippetSupport": false,
                            "documentationFormat": ["plaintext"],
                        }
                    },
                    "hover": { "contentFormat": ["plaintext"] },
                    "definition": {},
                    "publishDiagnostics": { "relatedInformation": true },
                },
                "workspace": {
                    "applyEdit": false,
                    "workspaceFolders": true,
                },
            },
            "trace": "off",
        }),
    )
}

pub fn initialized_notification() -> Value {
    make_notification("initialized", json!({}))
}

pub fn did_open_notification(uri: &str, text: &str, language_id: &str) -> Value {
    make_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        }),
    )
}

pub fn did_change_notification(uri: &str, text: &str, version: i32) -> Value {
    make_notification(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        }),
    )
}

pub fn did_close_notification(uri: &str) -> Value {
    make_notification(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": uri } }),
    )
}

pub fn completion_request(id: u64, uri: &str, line: u32, character: u32) -> Value {
    make_request(
        id,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": 1 },
        }),
    )
}

pub fn definition_request(id: u64, uri: &str, line: u32, character: u32) -> Value {
    make_request(
        id,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        }),
    )
}

pub fn hover_request(id: u64, uri: &str, line: u32, character: u32) -> Value {
    make_request(
        id,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        }),
    )
}

pub fn shutdown_request(id: u64) -> Value {
    make_request(id, "shutdown", Value::Null)
}

pub fn exit_notification() -> Value {
    make_notification("exit", Value::Null)
}

// ---------------------------------------------------------------------------
// LSP data types (parsed from server responses)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// Diagnostic severity levels (LSP spec §3.15.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

impl DiagnosticSeverity {
    pub fn from_u64(n: u64) -> Self {
        match n {
            1 => Self::Error,
            2 => Self::Warning,
            3 => Self::Information,
            _ => Self::Hint,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Info",
            Self::Hint => "Hint",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Error => "✖",
            Self::Warning => "⚠",
            Self::Information => "ℹ",
            Self::Hint => "·",
        }
    }
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.symbol())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

impl Diagnostic {
    pub fn from_value(v: &Value) -> Option<Self> {
        let range_v = v.get("range")?;
        let range = Range {
            start: Position {
                line: range_v["start"]["line"].as_u64()? as u32,
                character: range_v["start"]["character"].as_u64()? as u32,
            },
            end: Position {
                line: range_v["end"]["line"].as_u64()? as u32,
                character: range_v["end"]["character"].as_u64()? as u32,
            },
        };
        let severity = DiagnosticSeverity::from_u64(v["severity"].as_u64().unwrap_or(1));
        let message = v["message"].as_str()?.to_string();
        let source = v["source"].as_str().map(String::from);
        Some(Diagnostic { range, severity, message, source })
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}:{} {}",
            self.severity,
            self.range.start.line + 1,
            self.range.start.character + 1,
            self.message
        )
    }
}

/// Completion item kinds (LSP spec §3.18.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Keyword = 14,
    File = 17,
    Other = 255,
}

impl CompletionItemKind {
    pub fn from_u64(n: u64) -> Self {
        match n {
            1 => Self::Text,
            2 => Self::Method,
            3 => Self::Function,
            4 => Self::Constructor,
            5 => Self::Field,
            6 => Self::Variable,
            7 => Self::Class,
            8 => Self::Interface,
            9 => Self::Module,
            10 => Self::Property,
            14 => Self::Keyword,
            17 => Self::File,
            _ => Self::Other,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Method | Self::Function | Self::Constructor => "fn",
            Self::Class => "cl",
            Self::Interface => "if",
            Self::Variable | Self::Field => "vr",
            Self::Keyword => "kw",
            Self::Module => "md",
            Self::Property => "pr",
            Self::File => "fi",
            _ => "  ",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: String,
}

impl CompletionItem {
    pub fn from_value(v: &Value) -> Option<Self> {
        let label = v["label"].as_str()?.to_string();
        let kind = CompletionItemKind::from_u64(v["kind"].as_u64().unwrap_or(1));
        let detail = v["detail"].as_str().map(String::from);
        let documentation = match &v["documentation"] {
            Value::String(s) => Some(s.clone()),
            Value::Object(m) => m.get("value").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        };
        let insert_text = v["insertText"]
            .as_str()
            .unwrap_or(&label)
            .to_string();
        Some(CompletionItem { label, kind, detail, documentation, insert_text })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_content_length() {
        let msg = json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} });
        let encoded = encode_message(&msg);
        let header_end = encoded
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("header terminator");
        let header = &encoded[..header_end + 4];
        let body = &encoded[header_end + 4..];
        let length = parse_content_length(header).expect("content length");
        assert_eq!(length, body.len());
    }

    #[test]
    fn initialize_request_has_required_fields() {
        let req = initialize_request(1, "file:///project");
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["id"], 1);
        assert_eq!(req["method"], "initialize");
        assert_eq!(req["params"]["rootUri"], "file:///project");
    }

    #[test]
    fn diagnostic_from_value_severity_error() {
        let v = json!({
            "range": {
                "start": { "line": 0, "character": 5 },
                "end":   { "line": 0, "character": 10 }
            },
            "severity": 1,
            "message": "unresolved reference: foo",
        });
        let d = Diagnostic::from_value(&v).unwrap();
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.range.start.line, 0);
        assert_eq!(d.range.start.character, 5);
        assert!(d.message.contains("foo"));
    }

    #[test]
    fn diagnostic_from_value_defaults_to_error_severity() {
        let v = json!({
            "range": {
                "start": { "line": 1, "character": 0 },
                "end":   { "line": 1, "character": 1 }
            },
            "message": "test",
        });
        let d = Diagnostic::from_value(&v).unwrap();
        assert_eq!(d.severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn completion_item_from_value() {
        let v = json!({
            "label": "println",
            "kind": 3,
            "detail": "fun println(message: Any?)",
            "insertText": "println($0)",
        });
        let item = CompletionItem::from_value(&v).unwrap();
        assert_eq!(item.label, "println");
        assert_eq!(item.kind, CompletionItemKind::Function);
    }

    #[test]
    fn completion_item_insert_text_falls_back_to_label() {
        let v = json!({ "label": "myFun", "kind": 2 });
        let item = CompletionItem::from_value(&v).unwrap();
        assert_eq!(item.insert_text, "myFun");
    }

    #[test]
    fn diagnostic_severity_labels() {
        assert_eq!(DiagnosticSeverity::Error.label(), "Error");
        assert_eq!(DiagnosticSeverity::Warning.label(), "Warning");
        assert_eq!(DiagnosticSeverity::Information.label(), "Info");
        assert_eq!(DiagnosticSeverity::Hint.label(), "Hint");
    }

    #[test]
    fn make_notification_has_no_id() {
        let n = make_notification("initialized", json!({}));
        assert!(n.get("id").is_none());
        assert_eq!(n["jsonrpc"], "2.0");
    }
}
