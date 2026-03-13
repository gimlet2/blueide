# BlueIDE

A terminal-based Kotlin IDE inspired by the classic Turbo Pascal blue IDE. Works entirely in the terminal with Language Server Protocol (LSP) integration for Kotlin.

## Features

- 🔵 Classic blue Turbo Pascal-inspired terminal UI
- 📝 Code editor with Kotlin syntax highlighting
- 🗂️ File browser panel
- 🔗 Kotlin LSP integration (auto-complete, diagnostics, go-to-definition)
- ⌨️ Keyboard-driven interface
- 📊 Status bar with diagnostics

## Requirements

- Python 3.9+
- [kotlin-language-server](https://github.com/fwcd/kotlin-language-server) (optional, for LSP features)

## Installation

```bash
pip install blueide
```

Or from source:

```bash
pip install -e .
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
| `F3` | Open file |
| `F5` | Run / compile |
| `F10` | Activate menu bar |
| `Alt+F4` | Quit |
| `Ctrl+S` | Save |
| `Ctrl+O` | Open file |
| `Ctrl+Q` | Quit |
| `Ctrl+F` | Find |
| `Ctrl+Space` | Trigger completion |
| `F12` | Go to definition |
| `Ctrl+B` | Toggle file browser |

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
- Code completion
- Go-to-definition
- Hover documentation

## Development

```bash
# Install dev dependencies
pip install -e ".[dev]"

# Run tests
pytest

# Run with live reload
textual run --dev src/blueide/app.py
```