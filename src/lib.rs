//! BlueIDE library — public API for testing and downstream use.

pub mod editor;
pub mod lsp;

// Internal modules (not part of the public API).
pub(crate) mod app;
pub(crate) mod ui;
