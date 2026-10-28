//! BlueIDE — A terminal IDE with multi-language LSP support, inspired by Turbo Pascal.

mod app;
mod editor;
mod lsp;
mod ui;

use std::path::PathBuf;

use anyhow::Result;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // Simple argument parsing: optional path to file or directory.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let open_path = args.first().map(PathBuf::from);

    let mut application = App::new(open_path).await?;
    application.run().await?;
    Ok(())
}
