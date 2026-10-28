//! Integration tests for the LSP protocol helpers.

use blueide::lsp::protocol::{
    completion_request, did_change_notification, did_open_notification,
    encode_message, initialize_request, initialized_notification, make_notification,
    make_request, parse_content_length, shutdown_request, Diagnostic, DiagnosticSeverity,
    CompletionItem, CompletionItemKind,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

#[test]
fn encode_then_parse_roundtrip() {
    let payload = json!({"jsonrpc": "2.0", "method": "test", "params": {}});
    let encoded = encode_message(&payload);
    // Find header / body boundary.
    let sep = b"\r\n\r\n";
    let pos = encoded
        .windows(sep.len())
        .position(|w| w == sep)
        .expect("separator not found");
    let header = &encoded[..pos + sep.len()];
    let body = &encoded[pos + sep.len()..];
    let content_length = parse_content_length(header).expect("parse content-length");
    assert_eq!(content_length, body.len());
    let recovered: Value = serde_json::from_slice(body).unwrap();
    assert_eq!(recovered, payload);
}

#[test]
fn parse_content_length_case_insensitive() {
    let header = b"Content-Length: 42\r\n\r\n";
    assert_eq!(parse_content_length(header), Some(42));
    let header_lower = b"content-length: 99\r\n\r\n";
    assert_eq!(parse_content_length(header_lower), Some(99));
}

#[test]
fn parse_content_length_returns_none_for_missing() {
    let header = b"Accept: application/json\r\n\r\n";
    assert_eq!(parse_content_length(header), None);
}

// ---------------------------------------------------------------------------
// Message builders
// ---------------------------------------------------------------------------

#[test]
fn make_request_structure() {
    let req = make_request(7, "textDocument/hover", json!({"a": 1}));
    assert_eq!(req["jsonrpc"], "2.0");
    assert_eq!(req["id"], 7);
    assert_eq!(req["method"], "textDocument/hover");
    assert_eq!(req["params"]["a"], 1);
}

#[test]
fn make_notification_has_no_id_field() {
    let n = make_notification("initialized", json!({}));
    assert!(n.get("id").is_none(), "notifications must not have an id");
    assert_eq!(n["jsonrpc"], "2.0");
}

#[test]
fn initialize_request_contains_root_uri() {
    let req = initialize_request(1, "file:///workspace/project");
    assert_eq!(req["params"]["rootUri"], "file:///workspace/project");
    assert!(req["params"]["capabilities"].is_object());
}

#[test]
fn initialized_notification_method() {
    let n = initialized_notification();
    assert_eq!(n["method"], "initialized");
}

#[test]
fn did_open_notification_fields() {
    let n = did_open_notification("file:///a.kt", "fun main() {}", "kotlin");
    assert_eq!(n["method"], "textDocument/didOpen");
    assert_eq!(n["params"]["textDocument"]["uri"], "file:///a.kt");
    assert_eq!(n["params"]["textDocument"]["languageId"], "kotlin");
    assert_eq!(n["params"]["textDocument"]["version"], 1);
    assert_eq!(n["params"]["textDocument"]["text"], "fun main() {}");
}

#[test]
fn did_change_notification_fields() {
    let n = did_change_notification("file:///a.kt", "new content", 3);
    assert_eq!(n["method"], "textDocument/didChange");
    assert_eq!(n["params"]["textDocument"]["version"], 3);
    assert_eq!(
        n["params"]["contentChanges"][0]["text"],
        "new content"
    );
}

#[test]
fn completion_request_position() {
    let req = completion_request(2, "file:///a.kt", 10, 5);
    assert_eq!(req["method"], "textDocument/completion");
    assert_eq!(req["params"]["position"]["line"], 10);
    assert_eq!(req["params"]["position"]["character"], 5);
}

#[test]
fn shutdown_request_method() {
    let req = shutdown_request(99);
    assert_eq!(req["method"], "shutdown");
    assert_eq!(req["id"], 99);
}

// ---------------------------------------------------------------------------
// Diagnostic parsing
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_parses_all_severities() {
    for (sev_num, expected) in [
        (1u64, DiagnosticSeverity::Error),
        (2, DiagnosticSeverity::Warning),
        (3, DiagnosticSeverity::Information),
        (4, DiagnosticSeverity::Hint),
    ] {
        let v = json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "severity": sev_num,
            "message": "test",
        });
        let d = Diagnostic::from_value(&v).unwrap();
        assert_eq!(d.severity, expected, "severity {sev_num}");
    }
}

#[test]
fn diagnostic_display_shows_line_and_col() {
    let v = json!({
        "range": {"start": {"line": 4, "character": 11}, "end": {"line": 4, "character": 15}},
        "severity": 1,
        "message": "Unresolved reference: bar",
    });
    let d = Diagnostic::from_value(&v).unwrap();
    let s = d.to_string();
    assert!(s.contains("5:"), "expected 1-based line 5 in: {s}");
    assert!(s.contains(":12"), "expected 1-based col 12 in: {s}");
    assert!(s.contains("bar"), "expected message in: {s}");
}

#[test]
fn diagnostic_from_value_returns_none_on_missing_range() {
    let v = json!({ "severity": 1, "message": "oops" });
    assert!(Diagnostic::from_value(&v).is_none());
}

// ---------------------------------------------------------------------------
// CompletionItem parsing
// ---------------------------------------------------------------------------

#[test]
fn completion_item_kind_mapping() {
    for (num, expected) in [
        (2u64, CompletionItemKind::Method),
        (7, CompletionItemKind::Class),
        (14, CompletionItemKind::Keyword),
    ] {
        let v = json!({ "label": "foo", "kind": num });
        let item = CompletionItem::from_value(&v).unwrap();
        assert_eq!(item.kind, expected);
    }
}

#[test]
fn completion_item_documentation_string() {
    let v = json!({
        "label": "println",
        "kind": 3,
        "documentation": "Prints a value.",
    });
    let item = CompletionItem::from_value(&v).unwrap();
    assert_eq!(item.documentation.as_deref(), Some("Prints a value."));
}

#[test]
fn completion_item_documentation_object() {
    let v = json!({
        "label": "run",
        "kind": 3,
        "documentation": { "kind": "markdown", "value": "Runs the block." },
    });
    let item = CompletionItem::from_value(&v).unwrap();
    assert_eq!(item.documentation.as_deref(), Some("Runs the block."));
}
