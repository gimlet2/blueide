//! Application state and main event loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers,
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::editor::Editor;
use crate::lsp::{
    detect_language, resolve_server, CompletionItem, Diagnostic, Language, LspClient, LspEvent,
};

// ---------------------------------------------------------------------------
// Focus and mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusArea {
    Editor,
    Menu,
    Tree,
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Dialog {
    Help,
    About,
    OpenFile { input: String },
    SaveAs { input: String },
    Find { input: String, from_row: usize },
    Replace { find: String, replace_with: String, from_row: usize, focus_replace: bool },
    GoToLine { input: String },
    Message { text: String },
    Hover { text: String },
}

// ---------------------------------------------------------------------------
// Completion popup state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub enum CompletionState {
    #[default]
    Hidden,
    Visible {
        items: Vec<CompletionItem>,
        selected: usize,
    },
}

// ---------------------------------------------------------------------------
// File-tree entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub display: String,
}

// ---------------------------------------------------------------------------
// Help lines (used by ui.rs)
// ---------------------------------------------------------------------------

pub struct HelpLine {
    pub key: &'static str,
    pub action: &'static str,
}

// ---------------------------------------------------------------------------
// Submenu data — Turbo Pascal style dropdown entries
// ---------------------------------------------------------------------------

/// One entry in a dropdown submenu.  An empty `label` renders as a separator.
pub struct SubMenuItem {
    pub label: &'static str,
    pub shortcut: &'static str,
}

impl SubMenuItem {
    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }
}

const FILE_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "New",         shortcut: ""        },
    SubMenuItem { label: "Open...",     shortcut: "F3"      },
    SubMenuItem { label: "Save",        shortcut: "F2"      },
    SubMenuItem { label: "Save As...",  shortcut: ""        },
    SubMenuItem { label: "",            shortcut: ""        },
    SubMenuItem { label: "Exit",        shortcut: "Alt+X"   },
];

const EDIT_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Undo",        shortcut: "Ctrl+Z"  },
    SubMenuItem { label: "Redo",        shortcut: "Ctrl+Y"  },
    SubMenuItem { label: "",            shortcut: ""        },
    SubMenuItem { label: "Cut",         shortcut: "Ctrl+X"  },
    SubMenuItem { label: "Copy",        shortcut: "Ctrl+Ins"},
    SubMenuItem { label: "Paste",       shortcut: "Shift+Ins"},
    SubMenuItem { label: "",            shortcut: ""        },
    SubMenuItem { label: "Select All",  shortcut: "Ctrl+A"  },
];

const SEARCH_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Find...",       shortcut: "Ctrl+F"  },
    SubMenuItem { label: "Replace...",    shortcut: "Ctrl+H"  },
    SubMenuItem { label: "Find Again",    shortcut: "Ctrl+L"  },
    SubMenuItem { label: "",             shortcut: ""         },
    SubMenuItem { label: "Go to Line...", shortcut: "Ctrl+G"  },
];

const RUN_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Run",           shortcut: "F5"  },
    SubMenuItem { label: "Compile",       shortcut: "F9"  },
    SubMenuItem { label: "",             shortcut: ""     },
    SubMenuItem { label: "Parameters...", shortcut: ""    },
];

const OPTIONS_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Compiler...",    shortcut: "" },
    SubMenuItem { label: "Environment...", shortcut: "" },
    SubMenuItem { label: "",              shortcut: ""  },
    SubMenuItem { label: "Save Settings", shortcut: "" },
];

const WINDOW_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Toggle File Browser", shortcut: "Ctrl+B" },
    SubMenuItem { label: "",                    shortcut: ""       },
    SubMenuItem { label: "Refresh Display",     shortcut: ""       },
    SubMenuItem { label: "Close",               shortcut: "Alt+F3" },
];

const HELP_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Contents",     shortcut: "F1"       },
    SubMenuItem { label: "Index",        shortcut: "Shift+F1" },
    SubMenuItem { label: "Topic Search", shortcut: ""         },
    SubMenuItem { label: "",            shortcut: ""          },
    SubMenuItem { label: "About...",    shortcut: ""          },
];

/// Indexed by menu_index (0 = File … 6 = Help).
pub const SUBMENUS: &[&[SubMenuItem]] = &[
    FILE_MENU, EDIT_MENU, SEARCH_MENU, RUN_MENU, OPTIONS_MENU, WINDOW_MENU, HELP_MENU,
];

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub editor: Editor,
    pub focus: FocusArea,
    pub menu_index: usize,
    /// Whether a dropdown submenu is currently open.
    pub submenu_open: bool,
    /// Selected item index within the open submenu.
    pub submenu_index: usize,
    pub show_tree: bool,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub dialog: Option<Dialog>,
    pub completion: CompletionState,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub lsp_available: bool,
    pub status_msg: Option<String>,
    /// Internal line clipboard (copy/cut line operations).
    pub clipboard: String,
    /// Last search needle (for Find Again).
    pub last_needle: String,
    /// Detected language for the current editor buffer (None = unknown).
    pub current_language: Option<&'static Language>,
    /// Short description of how the LSP was launched ("native" / "docker" / "none").
    pub lsp_launch_mode: &'static str,
    /// Set to `true` after a file-open that changed the language; the event
    /// loop will restart the LSP and clear this flag.
    pending_lsp_restart: bool,
    lsp: LspClient,
    root_path: PathBuf,
}

impl App {
    pub async fn new(open_path: Option<PathBuf>) -> Result<Self> {
        let root_path = open_path
            .as_ref()
            .and_then(|p| if p.is_dir() { Some(p.clone()) } else { p.parent().map(|q| q.to_path_buf()) })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut editor = Editor::default();
        if let Some(ref p) = open_path {
            if p.is_file() {
                editor = Editor::load_file(p.clone()).unwrap_or_default();
            }
        }

        // Detect language from the initial file path (if any file was given).
        let initial_file = open_path.as_ref().filter(|p| p.is_file());
        let current_language = initial_file.and_then(|p| detect_language(p));

        // Resolve the LSP server command + track how we launched it.
        let root_str = root_path.to_string_lossy().into_owned();
        let (server_cmd, lsp_launch_mode) =
            resolve_server_command(current_language, &root_str);

        let lsp = LspClient::new(root_path.clone(), server_cmd);
        let tree_entries = build_tree(&root_path);

        Ok(Self {
            editor,
            focus: FocusArea::Editor,
            menu_index: 0,
            submenu_open: false,
            submenu_index: 0,
            show_tree: true,
            tree_entries,
            tree_selected: 0,
            dialog: None,
            completion: CompletionState::Hidden,
            diagnostics: HashMap::new(),
            lsp_available: false,
            status_msg: None,
            clipboard: String::new(),
            last_needle: String::new(),
            current_language,
            lsp_launch_mode,
            pending_lsp_restart: false,
            lsp,
            root_path,
        })
    }

    // ------------------------------------------------------------------
    // Main run loop
    // ------------------------------------------------------------------

    pub async fn run(&mut self) -> Result<()> {
        // Set up terminal.
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.hide_cursor()?;

        // Start LSP (non-blocking — if KLS not installed, just skip).
        let lsp_ok = self.lsp.start().await.unwrap_or(false);
        self.lsp_available = lsp_ok;

        // Notify LSP about the open file.
        if lsp_ok {
            let uri = self.editor.uri();
            let text = self.editor.text();
            let lang_id = self
                .current_language
                .map(|l| l.id)
                .unwrap_or("plaintext");
            let _ = self.lsp.open_file(&uri, &text, lang_id).await;
        }

        let result = self.event_loop(&mut terminal).await;

        // Restore terminal.
        self.lsp.stop().await;
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let tick = Duration::from_millis(50);

        loop {
            terminal.draw(|f| crate::ui::render(f, self))?;

            // Handle a deferred LSP restart (triggered by opening a file in a
            // different language).
            if self.pending_lsp_restart {
                self.pending_lsp_restart = false;
                self.restart_lsp().await;
            }

            // Drain any pending LSP events first (non-blocking).
            self.drain_lsp_events();

            // Wait for terminal event or timeout.
            if event::poll(tick)? {
                match event::read()? {
                    Event::Key(key) => {
                        if self.handle_key(key).await? {
                            break; // quit
                        }
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // LSP event drain
    // ------------------------------------------------------------------

    fn drain_lsp_events(&mut self) {
        let Some(rx) = self.lsp.events_rx.as_mut() else { return };
        // Drain all immediately available events.
        loop {
            match rx.try_recv() {
                Ok(LspEvent::Diagnostics { uri, diagnostics }) => {
                    self.diagnostics.insert(uri, diagnostics);
                }
                Ok(LspEvent::ServerExited) => {
                    self.lsp_available = false;
                }
                Err(_) => break,
            }
        }
    }

    // ------------------------------------------------------------------
    // Key handling
    // ------------------------------------------------------------------

    /// Returns `true` when the app should quit.
    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        // Dialog intercepts all keys.
        if self.dialog.is_some() {
            return self.handle_dialog_key(key).await;
        }

        // Completion popup navigation.
        if matches!(self.completion, CompletionState::Visible { .. }) {
            if self.handle_completion_key(key).await? {
                return Ok(false);
            }
        }

        // Alt+letter directly activates menu items from any focus area.
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if self.activate_menu_by_hotkey(c).await {
                    return Ok(false);
                }
            }
        }

        match self.focus {
            FocusArea::Menu => self.handle_menu_key(key).await,
            FocusArea::Tree => self.handle_tree_key(key),
            FocusArea::Editor => self.handle_editor_key(key).await,
        }
    }

    // ------------------------------------------------------------------
    // Editor keys
    // ------------------------------------------------------------------

    async fn handle_editor_key(&mut self, key: KeyEvent) -> Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            // ---- Quit ----
            KeyCode::Char('q') if ctrl => return Ok(true),
            KeyCode::Char('c') if ctrl => return Ok(true),
            KeyCode::F(4) if alt => return Ok(true),

            // ---- File operations ----
            KeyCode::F(2) | KeyCode::Char('s') if ctrl => self.save_file(),
            KeyCode::F(3) | KeyCode::Char('o') if ctrl => {
                let cur = self
                    .editor
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::OpenFile { input: cur });
            }

            // ---- Menu ----
            KeyCode::F(10) => {
                self.focus = FocusArea::Menu;
                self.menu_index = 0;
                self.submenu_open = false;
            }

            // ---- File browser toggle ----
            KeyCode::Char('b') if ctrl => {
                self.show_tree = !self.show_tree;
                if self.show_tree {
                    self.focus = FocusArea::Tree;
                }
            }

            // ---- Help ----
            KeyCode::F(1) => {
                self.dialog = Some(Dialog::Help);
            }

            // ---- Find ----
            KeyCode::Char('f') if ctrl => {
                self.dialog = Some(Dialog::Find {
                    input: self.last_needle.clone(),
                    from_row: self.editor.cursor_row,
                });
            }

            // ---- Find Again ----
            KeyCode::Char('l') if ctrl => {
                self.find_again();
            }

            // ---- Replace ----
            KeyCode::Char('h') if ctrl => {
                self.dialog = Some(Dialog::Replace {
                    find: self.last_needle.clone(),
                    replace_with: String::new(),
                    from_row: self.editor.cursor_row,
                    focus_replace: false,
                });
            }

            // ---- Go to Line ----
            KeyCode::Char('g') if ctrl => {
                self.dialog = Some(Dialog::GoToLine { input: String::new() });
            }

            // ---- LSP: completion ----
            KeyCode::Char(' ') if ctrl => {
                self.trigger_completion().await;
            }

            // ---- LSP: go to definition ----
            KeyCode::F(12) => {
                self.go_to_definition().await;
            }

            // ---- LSP: hover ----
            KeyCode::F(9) => {
                self.show_hover().await;
            }

            // ---- Undo / Redo ----
            KeyCode::Char('z') if ctrl => {
                self.editor.undo();
                self.notify_lsp_change().await;
            }
            KeyCode::Char('y') if ctrl => {
                self.editor.redo();
                self.notify_lsp_change().await;
            }

            // ---- Cut / Copy / Paste ----
            KeyCode::Char('x') if ctrl => {
                self.clipboard = self.editor.cut_line();
                self.notify_lsp_change().await;
            }
            KeyCode::Insert if ctrl => {
                self.clipboard = self.editor.copy_line();
            }
            KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.editor.insert_str(&text);
                    self.notify_lsp_change().await;
                }
            }

            // ---- Cursor movement ----
            KeyCode::Up => self.editor.move_up(),
            KeyCode::Down => self.editor.move_down(),
            KeyCode::Left if ctrl => self.editor.move_word_left(),
            KeyCode::Right if ctrl => self.editor.move_word_right(),
            KeyCode::Left => self.editor.move_left(),
            KeyCode::Right => self.editor.move_right(),
            KeyCode::Home => self.editor.move_home(),
            KeyCode::End => self.editor.move_end(),
            KeyCode::PageUp => self.editor.move_page_up(20),
            KeyCode::PageDown => self.editor.move_page_down(20),

            // ---- Text input ----
            KeyCode::Char(c) => {
                self.editor.insert_char(c);
                self.notify_lsp_change().await;
            }
            KeyCode::Enter => {
                self.editor.insert_newline();
                self.notify_lsp_change().await;
            }
            KeyCode::Backspace => {
                self.editor.delete_backward();
                self.notify_lsp_change().await;
            }
            KeyCode::Delete => {
                self.editor.delete_forward();
                self.notify_lsp_change().await;
            }
            KeyCode::Tab => {
                for _ in 0..4 {
                    self.editor.insert_char(' ');
                }
                self.notify_lsp_change().await;
            }

            KeyCode::Esc => {
                self.completion = CompletionState::Hidden;
            }

            _ => {}
        }
        Ok(false)
    }

    // ------------------------------------------------------------------
    // Menu keys
    // ------------------------------------------------------------------

    async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.submenu_open {
            // Navigate within the open dropdown.
            match key.code {
                KeyCode::Esc => {
                    self.submenu_open = false;
                }
                KeyCode::Up => {
                    let items = SUBMENUS[self.menu_index];
                    self.submenu_index = prev_selectable(items, self.submenu_index);
                }
                KeyCode::Down => {
                    let items = SUBMENUS[self.menu_index];
                    self.submenu_index = next_selectable(items, self.submenu_index);
                }
                KeyCode::Left => {
                    // Move to the previous top-level menu and open its submenu.
                    self.menu_index = if self.menu_index == 0 {
                        SUBMENUS.len() - 1
                    } else {
                        self.menu_index - 1
                    };
                    self.open_submenu();
                }
                KeyCode::Right => {
                    // Move to the next top-level menu and open its submenu.
                    self.menu_index = (self.menu_index + 1) % SUBMENUS.len();
                    self.open_submenu();
                }
                KeyCode::Enter => {
                    let menu_idx = self.menu_index;
                    let item_idx = self.submenu_index;
                    self.submenu_open = false;
                    let quit = self.activate_submenu_item(menu_idx, item_idx).await?;
                    // Return focus to editor unless the action changed it.
                    if self.focus == FocusArea::Menu {
                        self.focus = FocusArea::Editor;
                    }
                    return Ok(quit);
                }
                _ => {}
            }
        } else {
            // Navigate the menu bar (no dropdown open).
            match key.code {
                KeyCode::Esc | KeyCode::F(10) => {
                    self.focus = FocusArea::Editor;
                }
                KeyCode::Left => {
                    self.menu_index = if self.menu_index == 0 {
                        SUBMENUS.len() - 1
                    } else {
                        self.menu_index - 1
                    };
                }
                KeyCode::Right => {
                    self.menu_index = (self.menu_index + 1) % SUBMENUS.len();
                }
                KeyCode::Down | KeyCode::Enter => {
                    self.open_submenu();
                }
                // Hotkey letters on the bar open the matching submenu.
                KeyCode::Char(c) => {
                    if let Some(idx) = menu_hotkey_index(c) {
                        self.menu_index = idx;
                        self.open_submenu();
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    /// Open the dropdown for the currently active top-level menu.
    fn open_submenu(&mut self) {
        let items = SUBMENUS[self.menu_index];
        self.submenu_index = first_selectable(items);
        self.submenu_open = true;
    }

    /// Activate a submenu item by `(menu_index, item_index)`.
    /// Returns `true` if the application should quit.
    async fn activate_submenu_item(&mut self, menu_idx: usize, item_idx: usize) -> Result<bool> {
        let items = SUBMENUS.get(menu_idx).copied().unwrap_or(&[]);
        let label = match items.get(item_idx) {
            Some(i) if !i.is_separator() => i.label,
            _ => return Ok(false),
        };

        match (menu_idx, label) {
            // ── File ────────────────────────────────────────────────────
            (0, "New") => self.new_file(),
            (0, "Open...") => {
                let cur = self
                    .editor
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::OpenFile { input: cur });
            }
            (0, "Save") => self.save_file(),
            (0, "Save As...") => {
                let cur = self
                    .editor
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::SaveAs { input: cur });
            }
            (0, "Exit") => return Ok(true),

            // ── Edit ────────────────────────────────────────────────────
            (1, "Undo") => {
                self.editor.undo();
                self.notify_lsp_change().await;
            }
            (1, "Redo") => {
                self.editor.redo();
                self.notify_lsp_change().await;
            }
            (1, "Cut") => {
                self.clipboard = self.editor.cut_line();
                self.notify_lsp_change().await;
            }
            (1, "Copy") => {
                self.clipboard = self.editor.copy_line();
                self.status_msg = Some("Line copied.".into());
            }
            (1, "Paste") => {
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.editor.insert_str(&text);
                    self.notify_lsp_change().await;
                }
            }
            (1, "Select All") => {
                let last = self.editor.line_count().saturating_sub(1);
                self.editor.goto_line(last);
                self.editor.move_end();
                self.status_msg = Some("Cursor moved to end of file.".into());
            }

            // ── Search ──────────────────────────────────────────────────
            (2, "Find...") => {
                self.dialog = Some(Dialog::Find {
                    input: self.last_needle.clone(),
                    from_row: self.editor.cursor_row,
                });
            }
            (2, "Replace...") => {
                self.dialog = Some(Dialog::Replace {
                    find: self.last_needle.clone(),
                    replace_with: String::new(),
                    from_row: self.editor.cursor_row,
                    focus_replace: false,
                });
            }
            (2, "Find Again") => self.find_again(),
            (2, "Go to Line...") => {
                self.dialog = Some(Dialog::GoToLine { input: String::new() });
            }

            // ── Run ─────────────────────────────────────────────────────
            (3, "Run") | (3, "Compile") => self.run_project().await,
            (3, "Parameters...") => {
                self.dialog = Some(Dialog::Message {
                    text: "No run parameters configured.".into(),
                });
            }

            // ── Options ─────────────────────────────────────────────────
            (4, "Compiler...") => {
                let lang_info = match self.current_language {
                    Some(l) => format!(
                        "Language: {name}\nLSP server: {cmd}\nDocker image: {img}\nLaunch mode: {mode}",
                        name = l.name,
                        cmd = l.native_cmd,
                        img = l.docker_image,
                        mode = self.lsp_launch_mode,
                    ),
                    None => "No language detected for the current file.".into(),
                };
                self.dialog = Some(Dialog::Message { text: lang_info });
            }
            (4, "Environment...") => {
                let lsp_status = if self.lsp_available {
                    format!("LSP: active ({})", self.lsp_launch_mode)
                } else {
                    "LSP: inactive".into()
                };
                self.dialog = Some(Dialog::Message {
                    text: format!("Editor: BlueIDE\nTheme: Turbo Pascal Classic\n{lsp_status}"),
                });
            }
            (4, "Save Settings") => {
                self.status_msg = Some("Settings saved.".into());
            }

            // ── Window ───────────────────────────────────────────────────
            (5, "Toggle File Browser") => {
                self.show_tree = !self.show_tree;
                if self.show_tree {
                    self.focus = FocusArea::Tree;
                }
            }
            (5, "Refresh Display") => {
                self.status_msg = Some("Display refreshed.".into());
            }
            (5, "Close") => {
                // Close current buffer (new empty file).
                self.new_file();
            }

            // ── Help ─────────────────────────────────────────────────────
            (6, "Contents") | (6, "Index") | (6, "Topic Search") => {
                self.dialog = Some(Dialog::Help);
            }
            (6, "About...") => {
                self.dialog = Some(Dialog::About);
            }

            _ => {}
        }
        Ok(false)
    }

    /// Activate a menu dropdown by its Alt+letter hotkey.
    ///
    /// Opens the matching submenu so the user can pick a sub-item.
    /// Returns `true` when a matching item was found.
    async fn activate_menu_by_hotkey(&mut self, ch: char) -> bool {
        let Some(idx) = menu_hotkey_index(ch) else {
            return false;
        };
        self.menu_index = idx;
        self.focus = FocusArea::Menu;
        self.open_submenu();
        true
    }

    // ------------------------------------------------------------------
    // Tree keys
    // ------------------------------------------------------------------

    fn handle_tree_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => {
                self.focus = FocusArea::Editor;
            }
            KeyCode::Up => {
                self.tree_selected = self.tree_selected.saturating_sub(1);
            }
            KeyCode::Down => {
                self.tree_selected =
                    (self.tree_selected + 1).min(self.tree_entries.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(entry) = self.tree_entries.get(self.tree_selected) {
                    let path = entry.path.clone();
                    if path.is_file() {
                        self.open_file_path(path);
                        self.focus = FocusArea::Editor;
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    // ------------------------------------------------------------------
    // Dialog keys
    // ------------------------------------------------------------------

    async fn handle_dialog_key(&mut self, key: KeyEvent) -> Result<bool> {
        let dialog = self.dialog.take();
        match dialog {
            Some(Dialog::Help) | Some(Dialog::About) => {
                // Any key closes.
            }
            Some(Dialog::OpenFile { mut input }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        let path = PathBuf::from(&input);
                        self.open_file_path(path);
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        self.dialog = Some(Dialog::OpenFile { input });
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        self.dialog = Some(Dialog::OpenFile { input });
                    }
                    _ => {
                        self.dialog = Some(Dialog::OpenFile { input });
                    }
                }
            }
            Some(Dialog::SaveAs { mut input }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        let path = PathBuf::from(&input);
                        match self.editor.save_as(path) {
                            Ok(_) => {
                                self.status_msg = Some("Saved.".into());
                                self.tree_entries = build_tree(&self.root_path);
                            }
                            Err(e) => {
                                self.dialog =
                                    Some(Dialog::Message { text: format!("Save failed: {e}") });
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        self.dialog = Some(Dialog::SaveAs { input });
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        self.dialog = Some(Dialog::SaveAs { input });
                    }
                    _ => {
                        self.dialog = Some(Dialog::SaveAs { input });
                    }
                }
            }
            Some(Dialog::Find { mut input, from_row }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        self.find_text(&input, from_row);
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        self.dialog = Some(Dialog::Find { input, from_row });
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        self.dialog = Some(Dialog::Find { input, from_row });
                    }
                    _ => {
                        self.dialog = Some(Dialog::Find { input, from_row });
                    }
                }
            }
            Some(Dialog::Replace { mut find, mut replace_with, from_row, mut focus_replace }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Tab => {
                        focus_replace = !focus_replace;
                        self.dialog = Some(Dialog::Replace { find, replace_with, from_row, focus_replace });
                    }
                    KeyCode::Enter => {
                        if !focus_replace {
                            // Move focus to the replace field.
                            self.dialog = Some(Dialog::Replace { find, replace_with, from_row, focus_replace: true });
                        } else {
                            // Execute the replacement.
                            self.replace_one(&find, &replace_with, from_row);
                        }
                    }
                    KeyCode::Backspace => {
                        if focus_replace { replace_with.pop(); } else { find.pop(); }
                        self.dialog = Some(Dialog::Replace { find, replace_with, from_row, focus_replace });
                    }
                    KeyCode::Char(c) => {
                        if focus_replace { replace_with.push(c); } else { find.push(c); }
                        self.dialog = Some(Dialog::Replace { find, replace_with, from_row, focus_replace });
                    }
                    _ => {
                        self.dialog = Some(Dialog::Replace { find, replace_with, from_row, focus_replace });
                    }
                }
            }
            Some(Dialog::GoToLine { mut input }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        match input.trim().parse::<usize>() {
                            Ok(n) if n >= 1 => {
                                self.editor.goto_line(n - 1); // input is 1-based
                            }
                            _ => {
                                self.dialog = Some(Dialog::Message {
                                    text: "Invalid line number.".into(),
                                });
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        self.dialog = Some(Dialog::GoToLine { input });
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        self.dialog = Some(Dialog::GoToLine { input });
                    }
                    _ => {
                        self.dialog = Some(Dialog::GoToLine { input });
                    }
                }
            }
            Some(Dialog::Message { .. }) | Some(Dialog::Hover { .. }) => {
                // Any key closes.
            }
            None => {}
        }
        Ok(false)
    }

    // ------------------------------------------------------------------
    // Completion popup key handling
    // ------------------------------------------------------------------

    /// Returns `true` if the key was consumed by the completion popup.
    async fn handle_completion_key(&mut self, key: KeyEvent) -> Result<bool> {
        let CompletionState::Visible { ref items, ref mut selected } = self.completion else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Up => {
                if *selected > 0 {
                    *selected -= 1;
                }
                return Ok(true);
            }
            KeyCode::Down => {
                if *selected + 1 < items.len() {
                    *selected += 1;
                }
                return Ok(true);
            }
            KeyCode::Enter | KeyCode::Tab => {
                let text = items[*selected].insert_text.clone();
                self.completion = CompletionState::Hidden;
                self.editor.insert_str(&text);
                self.notify_lsp_change().await;
                return Ok(true);
            }
            KeyCode::Esc => {
                self.completion = CompletionState::Hidden;
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    // ------------------------------------------------------------------
    // File operations
    // ------------------------------------------------------------------

    fn save_file(&mut self) {
        if self.editor.file_path.is_none() {
            self.dialog = Some(Dialog::SaveAs { input: String::new() });
            return;
        }
        match self.editor.save() {
            Ok(_) => self.status_msg = Some("Saved.".into()),
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    text: format!("Save failed: {e}"),
                });
            }
        }
    }

    fn new_file(&mut self) {
        self.editor = Editor::default();
        self.status_msg = Some("New file.".into());
    }

    fn open_file_path(&mut self, path: PathBuf) {
        match Editor::load_file(path.clone()) {
            Ok(ed) => {
                self.editor = ed;
                // Re-detect language — may need a new LSP server.
                let new_lang = detect_language(&path);
                let lang_changed = new_lang.map(|l| l.id) != self.current_language.map(|l| l.id);
                self.current_language = new_lang;
                if lang_changed {
                    // Signal that the LSP must be restarted on the next run()
                    // iteration.  We set a status message and mark the LSP as
                    // unavailable; the actual restart happens the next time the
                    // file is opened via `restart_lsp_for_language`.
                    self.lsp_available = false;
                    let lang_name = new_lang
                        .map(|l| l.name)
                        .unwrap_or("unknown");
                    self.status_msg = Some(format!(
                        "Opened · Language: {lang_name} — restarting LSP…"
                    ));
                    // Schedule a deferred LSP restart (picked up in event_loop).
                    self.pending_lsp_restart = true;
                } else {
                    self.status_msg = Some("File opened.".into());
                }
            }
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    text: format!("Open failed: {e}"),
                });
            }
        }
    }

    fn find_text(&mut self, needle: &str, from_row: usize) {
        if needle.is_empty() {
            return;
        }
        self.last_needle = needle.to_string();
        let lines = self.editor.lines().to_vec();
        let n = lines.len();
        for offset in 0..n {
            let row = (from_row + offset) % n;
            if let Some(col) = lines[row].find(needle) {
                self.editor.cursor_row = row;
                self.editor.cursor_col = col;
                return;
            }
        }
        self.dialog = Some(Dialog::Message {
            text: format!("'{needle}' not found"),
        });
    }

    /// Search for the last used needle starting from the line after the cursor.
    fn find_again(&mut self) {
        if self.last_needle.is_empty() {
            self.dialog = Some(Dialog::Find {
                input: String::new(),
                from_row: self.editor.cursor_row,
            });
            return;
        }
        let needle = self.last_needle.clone();
        // Start from the next line to avoid immediately re-matching same position.
        let from = (self.editor.cursor_row + 1) % self.editor.line_count().max(1);
        self.find_text(&needle, from);
    }

    /// Replace the next occurrence of `needle` with `replacement`.
    fn replace_one(&mut self, needle: &str, replacement: &str, from_row: usize) {
        self.last_needle = needle.to_string();
        if !self.editor.replace_next(needle, replacement, from_row) {
            self.dialog = Some(Dialog::Message {
                text: format!("'{needle}' not found"),
            });
        }
    }

    // ------------------------------------------------------------------
    // LSP helpers
    // ------------------------------------------------------------------

    /// Stop the current LSP, build a new server command for `current_language`,
    /// create a fresh [`LspClient`], start it, and notify it about the open file.
    async fn restart_lsp(&mut self) {
        // Gracefully stop the old server (if running).
        self.lsp.stop().await;
        self.lsp_available = false;
        self.diagnostics.clear();

        let root_str = self.root_path.to_string_lossy().into_owned();
        let (server_cmd, launch_mode) =
            resolve_server_command(self.current_language, &root_str);
        self.lsp_launch_mode = launch_mode;

        // Create a brand-new client.
        self.lsp = LspClient::new(self.root_path.clone(), server_cmd);

        let lsp_ok = self.lsp.start().await.unwrap_or(false);
        self.lsp_available = lsp_ok;

        if lsp_ok {
            let uri = self.editor.uri();
            let text = self.editor.text();
            let lang_id = self
                .current_language
                .map(|l| l.id)
                .unwrap_or("plaintext");
            let _ = self.lsp.open_file(&uri, &text, lang_id).await;

            let lang_name = self
                .current_language
                .map(|l| l.name)
                .unwrap_or("unknown");
            self.status_msg = Some(format!(
                "LSP started ({launch_mode}) for {lang_name}"
            ));
        } else {
            let lang_name = self
                .current_language
                .map(|l| l.name)
                .unwrap_or("unknown");
            self.status_msg = Some(format!(
                "No LSP available for {lang_name} — install {cmd} or Docker",
                cmd = self
                    .current_language
                    .map(|l| l.native_cmd)
                    .unwrap_or("language server"),
            ));
        }
    }

    async fn notify_lsp_change(&mut self) {
        if !self.lsp_available {
            return;
        }
        let uri = self.editor.uri();
        if uri.is_empty() {
            return;
        }
        let text = self.editor.text();
        let _ = self.lsp.change_file(&uri, &text).await;
    }

    async fn trigger_completion(&mut self) {
        if !self.lsp_available {
            return;
        }
        let uri = self.editor.uri();
        if uri.is_empty() {
            return;
        }
        let row = self.editor.cursor_row as u32;
        let col = self.editor.cursor_col as u32;
        match self.lsp.complete(&uri, row, col).await {
            Ok(items) if !items.is_empty() => {
                self.completion = CompletionState::Visible { items, selected: 0 };
            }
            Ok(_) => {
                self.status_msg = Some("No completions available.".into());
            }
            Err(e) => {
                self.status_msg = Some(format!("Completion error: {e}"));
            }
        }
    }

    async fn go_to_definition(&mut self) {
        if !self.lsp_available {
            return;
        }
        let uri = self.editor.uri();
        if uri.is_empty() {
            return;
        }
        let row = self.editor.cursor_row as u32;
        let col = self.editor.cursor_col as u32;
        match self.lsp.definition(&uri, row, col).await {
            Ok(locs) if !locs.is_empty() => {
                let loc = &locs[0];
                let target_uri = &loc.uri;
                let target_row = loc.range.start.line as usize;
                let target_col = loc.range.start.character as usize;

                // Strip file:// prefix.
                let path_str = target_uri
                    .strip_prefix("file://")
                    .unwrap_or(target_uri);
                let path = PathBuf::from(path_str);

                if path != self.editor.file_path.clone().unwrap_or_default() {
                    match Editor::load_file(path) {
                        Ok(ed) => {
                            self.editor = ed;
                        }
                        Err(e) => {
                            self.dialog = Some(Dialog::Message {
                                text: format!("Cannot open: {e}"),
                            });
                            return;
                        }
                    }
                }
                self.editor.cursor_row = target_row;
                self.editor.cursor_col = target_col;
            }
            Ok(_) => {
                self.status_msg = Some("No definition found.".into());
            }
            Err(e) => {
                self.status_msg = Some(format!("Definition error: {e}"));
            }
        }
    }

    async fn show_hover(&mut self) {
        if !self.lsp_available {
            return;
        }
        let uri = self.editor.uri();
        if uri.is_empty() {
            return;
        }
        let row = self.editor.cursor_row as u32;
        let col = self.editor.cursor_col as u32;
        match self.lsp.hover(&uri, row, col).await {
            Ok(text) if !text.trim().is_empty() => {
                self.dialog = Some(Dialog::Hover { text });
            }
            Ok(_) => {
                self.status_msg = Some("No hover info available.".into());
            }
            Err(e) => {
                self.status_msg = Some(format!("Hover error: {e}"));
            }
        }
    }

    // ------------------------------------------------------------------
    // Run / compile
    // ------------------------------------------------------------------

    async fn run_project(&mut self) {
        // Find build script or main .kt file.
        let build_file = ["gradlew", "build.gradle.kts", "build.gradle"]
            .iter()
            .map(|f| self.root_path.join(f))
            .find(|p| p.exists());

        let text = if let Some(ref bf) = build_file {
            let gradle = self.root_path.join("gradlew");
            let cmd = if gradle.exists() { "./gradlew run" } else { "gradle run" };
            format!("Run: {cmd}\n(in {})", bf.parent().unwrap_or(&self.root_path).display())
        } else if let Some(ref fp) = self.editor.file_path.clone() {
            format!("Run: kotlinc {} -include-runtime -d out.jar && java -jar out.jar", fp.display())
        } else {
            "No runnable project or file found.".into()
        };

        self.dialog = Some(Dialog::Message { text });
    }
}

// ---------------------------------------------------------------------------
// LSP server resolution helper
// ---------------------------------------------------------------------------

/// Build the server argv and a human-readable launch-mode label for a given
/// optional language.  Returns `(None, "none")` when no server is available.
fn resolve_server_command(
    lang: Option<&'static Language>,
    workspace: &str,
) -> (Option<Vec<String>>, &'static str) {
    let Some(lang) = lang else {
        return (None, "none");
    };
    match resolve_server(lang, workspace) {
        Some(launch @ crate::lsp::ServerLaunch::Native { .. }) => {
            (Some(launch.into_argv()), "native")
        }
        Some(launch @ crate::lsp::ServerLaunch::Docker { .. }) => {
            (Some(launch.into_argv()), "docker")
        }
        None => (None, "none"),
    }
}

// ---------------------------------------------------------------------------
// Tree builder
// ---------------------------------------------------------------------------

/// Build a flat list of file-tree entries for `root`, depth ≤ 2.
pub fn build_tree(root: &Path) -> Vec<TreeEntry> {
    let mut entries = Vec::new();
    collect_tree(root, root, 0, &mut entries);
    entries
}

fn collect_tree(root: &Path, dir: &Path, depth: usize, out: &mut Vec<TreeEntry>) {
    if depth > 2 {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<std::fs::DirEntry> = read_dir.flatten().collect();
    children.sort_by_key(|e| {
        let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
        (is_file, e.file_name())
    });

    for entry in children {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden files and common noise directories.
        if name_str.starts_with('.') {
            continue;
        }
        if matches!(
            name_str.as_ref(),
            "target" | "node_modules" | ".git" | "build" | ".gradle" | "__pycache__"
        ) {
            continue;
        }

        let indent = "  ".repeat(depth);
        let prefix = if path.is_dir() { "▸ " } else { "  " };
        let display = format!("{indent}{prefix}{name_str}");
        out.push(TreeEntry { path: path.clone(), display });

        if path.is_dir() {
            collect_tree(root, &path, depth + 1, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Submenu navigation helpers
// ---------------------------------------------------------------------------

/// Return the menu index for a hotkey character, or `None`.
fn menu_hotkey_index(ch: char) -> Option<usize> {
    match ch.to_ascii_lowercase() {
        'f' => Some(0), // File
        'e' => Some(1), // Edit
        's' => Some(2), // Search
        'r' => Some(3), // Run
        'o' => Some(4), // Options
        'w' => Some(5), // Window
        'h' => Some(6), // Help
        _ => None,
    }
}

/// Return the index of the first non-separator item in `items`.
fn first_selectable(items: &[SubMenuItem]) -> usize {
    items.iter().position(|i| !i.is_separator()).unwrap_or(0)
}

/// Move selection down, skipping separators.
fn next_selectable(items: &[SubMenuItem], current: usize) -> usize {
    let mut next = current + 1;
    while next < items.len() {
        if !items[next].is_separator() {
            return next;
        }
        next += 1;
    }
    current
}

/// Move selection up, skipping separators.
fn prev_selectable(items: &[SubMenuItem], current: usize) -> usize {
    if current == 0 {
        return current;
    }
    let mut prev = current - 1;
    loop {
        if !items[prev].is_separator() {
            return prev;
        }
        if prev == 0 {
            break;
        }
        prev -= 1;
    }
    current
}
