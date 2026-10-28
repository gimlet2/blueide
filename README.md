# BlueIDE

A terminal-based multi-language IDE inspired by the classic Turbo Pascal blue IDE. Works entirely in the terminal with automatic Language Server Protocol (LSP) detection for 18 programming languages — native binary first, with optional Docker fallback.

## Features

- 🔵 Classic blue Turbo Pascal-inspired terminal UI
- 📝 Code editor with syntax highlighting (Kotlin, Rust, and more)
- 🗂️ File browser panel
- 🔗 Multi-language LSP integration — auto-detect language from file extension, launch the right server automatically
- 🐳 Docker fallback — if the native LSP binary is not installed, BlueIDE will try to run it inside Docker
- ⌨️ Keyboard-driven interface (Turbo Pascal-style dropdown menus)
- 🛠️ Compile and run recipes with diagnostic navigation
- 🐞 GDB TUI debugging for Rust, C, C++, and Free Pascal
- 📑 Multiple open buffers, collapsible file tree, and block selection
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
| Pascal         | `.pas`, `.pp`, `.p`, `.lpr`, `.dpr` | `pasls`                | Native only                        |
| Lua            | `.lua`                    | `lua-language-server`         | `blueide/lua-ls:latest`            |
| Shell          | `.sh`, `.bash`            | `bash-language-server`        | `blueide/bash-ls:latest`           |
| YAML           | `.yaml`, `.yml`           | `yaml-language-server`        | `blueide/yaml-ls:latest`           |
| JSON           | `.json`, `.jsonc`         | `vscode-json-language-server` | `blueide/vscode-ls:latest`         |
| HTML           | `.html`, `.htm`           | `vscode-html-language-server` | `blueide/vscode-ls:latest`         |
| CSS/SCSS       | `.css`, `.scss`, `.sass`  | `vscode-css-language-server`  | `blueide/vscode-ls:latest`         |
| TOML           | `.toml`                   | `taplo`                       | `blueide/taplo:latest`             |
| Markdown       | `.md`                     | `marksman`                    | `blueide/marksman:latest`          |

If a native binary is not installed **and** Docker is available, BlueIDE will automatically pull and run the server inside a Docker container with `--network=none` for security. Pascal uses `pasls` from PATH and currently has no Docker fallback image configured.

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
| `F5` | Toggle file browser |
| `F6` | Hover info (LSP) |
| `F8` / `Shift+F8` | Toggle breakpoint / start GDB |
| `F9` | Compile |
| `Ctrl+F9` | Run |
| `Ctrl+Tab` | Switch open buffer |
| `Ctrl+PageUp` / `Ctrl+PageDown` | Previous / next buffer |
| `Ctrl+1`–`Ctrl+9` | Jump to a visible buffer |
| `F10` | Activate menu bar |
| `F12` | Go to definition (LSP) |
| `Alt+F4` / `Ctrl+Q` | Quit |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+X` | Cut selection or line |
| `Ctrl+Ins` | Copy selection or line |
| `Shift+Ins` | Paste |
| `Shift+Arrow` | Extend block selection |
| `Ctrl+A` | Select all |
| `Ctrl+F` | Find |
| `Ctrl+L` | Find Again |
| `Ctrl+H` | Replace |
| `Ctrl+G` | Go to Line |
| `Ctrl+Space` | Trigger completion (LSP) |
| `Ctrl+B` | Toggle file browser |

Mouse clicks select menu items, buffers, and files; click in the editor to place the cursor, drag to select text, and use the wheel to scroll.

## LSP Setup

BlueIDE automatically detects the file language and tries to launch the appropriate server.

**Priority order:**
1. Native binary in your `$PATH`
2. Docker container (if `docker` is available in `$PATH`)
3. LSP disabled (editing still works, just without LSP features)

You can verify what was detected in the status bar (shows `[Language]` or `[Language·🐳]` for Docker) or via `Options → Compiler...` in the menu.

Pascal files use `pasls` when it is installed. Install Free Pascal (`fpc`) to compile and run Pascal files; GDB is used for native debugging when available. `Shift+F8` starts the native GDB TUI for Rust, C, C++, and Pascal. Set breakpoints with `F8`; GDB provides stepping, watches (`print`), and the call stack (`backtrace`).

Compile and run use `F9` and `Ctrl+F9`. Compiler output appears in an IDE dialog; select a diagnostic and press Enter to jump to its source location. Run temporarily returns to the terminal so programs can read input. Compiler overrides and run parameters are configured under Options and saved in the project root as `.blueide.json`.

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
