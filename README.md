# BlueIDE

A terminal-based Kotlin IDE inspired by the classic Turbo Pascal blue IDE. Works entirely in the terminal with Language Server Protocol (LSP) integration for Kotlin.

## Features

- 🔵 Classic blue Turbo Pascal-inspired terminal UI
- 📝 Code editor with Kotlin syntax highlighting
- 🗂️ File browser panel
- 🔗 Kotlin LSP integration (auto-complete, diagnostics, go-to-definition)
- ⌨️ Keyboard-driven interface
- 📊 Status bar with diagnostics

![BlueIDE screenshot](https://github.com/user-attachments/assets/5c198e68-5d0f-40fb-8ce7-b3c420ba46ec)

## Requirements

- Rust 1.70+ (edition 2021)
- [kotlin-language-server](https://github.com/fwcd/kotlin-language-server) (optional, for LSP features)

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

# Open a specific file
blueide path/to/file.kt

# Open a specific directory
blueide path/to/project/
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `F1` | Help |
| `F2` | Save |
| `F3` / `Ctrl+O` | Open file |
| `F5` | Run / compile |
| `F10` | Activate menu bar |
| `Alt+F4` / `Ctrl+Q` | Quit |
| `Ctrl+S` | Save |
| `Ctrl+F` | Find |
| `Ctrl+Space` | Trigger completion |
| `F12` | Go to definition |
| `F9` | Show hover info |
| `Ctrl+B` | Toggle file browser |
| `Ctrl+Z` | Undo |

## LSP Setup

Install the [Kotlin Language Server](https://github.com/fwcd/kotlin-language-server):

```bash
# Via Homebrew (macOS/Linux)
brew install kotlin-language-server

# Or download the latest release from:
# https://github.com/fwcd/kotlin-language-server/releases
```

BlueIDE will automatically detect `kotlin-language-server` in your PATH and use it for:
- Real-time diagnostics (errors, warnings)
- Code completion (`Ctrl+Space`)
- Go-to-definition (`F12`)
- Hover documentation (`F9`)

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