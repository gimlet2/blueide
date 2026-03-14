//! LSP module — re-exports the client and protocol types.

pub mod client;
pub mod language;
pub mod protocol;

pub use client::{url_from_path, LspClient, LspEvent};
pub use language::{detect_language, resolve_server, Language, ServerLaunch};
pub use protocol::{CompletionItem, Diagnostic, DiagnosticSeverity};
