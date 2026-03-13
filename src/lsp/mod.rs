//! LSP module — re-exports the client and protocol types.

pub mod client;
pub mod protocol;

pub use client::{url_from_path, LspClient, LspEvent};
pub use protocol::{CompletionItem, Diagnostic, DiagnosticSeverity};
