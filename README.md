# BlueIDE

A terminal-based multi-language IDE inspired by the classic Turbo Pascal blue IDE. Works entirely in the terminal with automatic Language Server Protocol (LSP) detection for 17 programming languages — native binary first, with optional Docker fallback.

## Features

- 🔵 Classic blue Turbo Pascal-inspired terminal UI
- 📝 Code editor with syntax highlighting (Kotlin, Rust, and more)
- 🗂️ File browser panel
- 🔗 Multi-language LSP integration — auto-detect language from file extension, launch the right server automatically
- 🐳 Docker fallback — if the native LSP binary is not installed, BlueIDE will try to run it inside Docker
- ⌨️ Keyboard-driven interface (Turbo Pascal-style dropdown menus)
- 📊 Status bar with diagnostics and language indicator

![BlueIDE screenshot](https://github.com/user-attachments/assets/5c198e68-5d0f-40fb-8ce7-b3c420ba46ec)

## Supported Languages

BlueIDE detects the language from the file extension and automatically starts the appropriate LSP server:

| Language       | Extension(s)              | Native LSP binary              | Docker image                        |
|----------------|---------------------------|-------------------------------|-------------------------------------|
| Kotlin         | `.kt`, `.kts`             | `kotlin-language-server`      | `fwcd/kotlin-language-server:latest`|
| Rust           | `.rs`                     | `rust-analyzer`               | `ghcr.io/rust-lang/rust-analyzer`  |
| Python         | `.py`, `.pyw`             | `pyright-langserver`          | `blueide/pyright:latest`           |
| TypeScript     | `.ts`, `.tsx`             | `typescript-language-server`  | `blueide/typescript-ls:latest`     |
| JavaScript     | `.js`, `.mjs`, `.jsx`     | `typescript-language-server`  | `blueide/typescript-ls:latest`     |
| Go             | `.go`                     | `gopls`                       | `golang:latest`                    |
| C              | `.c`, `.h`                | `clangd`                      | `blueide/clangd:latest`            |
| C++            | `.cpp`, `.cc`, `.hpp`     | `clangd`                      | `blueide/clangd:latest`            |
| Java           | `.java`                   | `jdtls`                       | `blueide/jdtls:latest`             |
| Lua            | `.lua`                    | `lua-language-server`         | `blueide/lua-ls:latest`            |
| Shell          | `.sh`, `.bash`            | `bash-language-server`        | `blueide/bash-ls:latest`           |
| YAML           | `.yaml`, `.yml`           | `yaml-language-server`        | `blueide/yaml-ls:latest`           |
| JSON           | `.json`, `.jsonc`         | `vscode-json-language-server` | `blueide/vscode-ls:latest`         |
| HTML           | `.html`, `.htm`           | `vscode-html-language-server` | `blueide/vscode-ls:latest`         |
| CSS/SCSS       | `.css`, `.scss`, `.sass`  | `vscode-css-language-server`  | `blueide/vscode-ls:latest`         |
| TOML           | `.toml`                   | `taplo`                       | `blueide/taplo:latest`             |
| Markdown       | `.md`                     | `marksman`                    | `blueide/marksman:latest`          |

If a native binary is not installed **and** Docker is available, BlueIDE will automatically pull and run the server inside a Docker container with `--network=none` for security.

## Requirements

- Rust 1.70+ (edition 2021)
- A language server for your language (see table above), **or** Docker

## Installation

```bash
cargo install --git https://github.com/gimlet2/blueide
```

Or from source:

```bash
git clone https://github.com/gimlet2/blueide
cd blueide
cargo build --release
./target/release/blueide
```

## Usage

```bash
# Open the IDE in the current directory
blueide

# Open a specific file (language is auto-detected from extension)
blueide path/to/file.kt
blueide path/to/main.rs
blueide path/to/script.py

# Open a specific directory
blueide path/to/project/
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `F1` | Help |
| `F2` / `Ctrl+S` | Save |
| `F3` / `Ctrl+O` | Open file |
| `F5` | Run / compile |
| `F9` | Hover info (LSP) |
| `F10` | Activate menu bar |
| `F12` | Go to definition (LSP) |
| `Alt+F4` / `Ctrl+Q` | Quit |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+X` | Cut line |
| `Ctrl+Ins` | Copy line |
| `Shift+Ins` | Paste |
| `Ctrl+F` | Find |
| `Ctrl+L` | Find Again |
| `Ctrl+H` | Replace |
| `Ctrl+G` | Go to Line |
| `Ctrl+Space` | Trigger completion (LSP) |
| `Ctrl+B` | Toggle file browser |

## LSP Setup

BlueIDE automatically detects the file language and tries to launch the appropriate server.

**Priority order:**
1. Native binary in your `$PATH`
2. Docker container (if `docker` is available in `$PATH`)
3. LSP disabled (editing still works, just without LSP features)

You can verify what was detected in the status bar (shows `[Language]` or `[Language·🐳]` for Docker) or via `Options → Compiler...` in the menu.

## Development

```bash
# Run tests
cargo test

# Build debug binary
cargo build

# Build optimised release binary
cargo build --release

# Run with a specific file
cargo run -- path/to/file.kt
```