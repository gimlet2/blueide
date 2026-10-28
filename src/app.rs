//! Application state and main event loop.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};

use crate::editor::Editor;
use crate::lsp::{
    detect_language, resolve_server, CompletionItem, Diagnostic, Language, LspClient, LspEvent,
};

const AUTOSAVE_IDLE: Duration = Duration::from_millis(900);

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
    Hover { text: String, scroll: u16 },
    DockerApproval { image: String },
    GitChanges { changes: Vec<GitChange>, selected: usize, scroll: usize },
    GitCommit { changes: Vec<GitChange>, selected: usize, scroll: usize, message: String },
    GitBranches { branches: Vec<String>, current: String, selected: usize },
    GitNewBranch { branches: Vec<String>, current: String, selected: usize, name: String },
    GitLog {
        entries: Vec<GitCommitEntry>,
        selected: usize,
        message: String,
        files: Vec<GitLogFile>,
        selected_file: usize,
        file_focus: bool,
        expanded_file: Option<usize>,
        scroll: usize,
    },
    ConfirmDiscard { action: PendingFileAction },
    BuildOutput {
        title: String,
        lines: Vec<String>,
        diagnostics: Vec<BuildDiagnostic>,
        selected: usize,
    },
    SettingsInput { kind: SettingKind, input: String },
}

#[derive(Debug, Clone)]
pub enum SettingKind {
    Parameters,
    Compiler(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProjectSettings {
    #[serde(default)]
    run_parameters: String,
    #[serde(default)]
    compiler_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct BuildDiagnostic {
    pub output_line: usize,
    pub path: PathBuf,
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct GitChange {
    pub path: PathBuf,
    pub index_status: char,
    pub worktree_status: char,
    pub diff: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GitCommitEntry {
    pub hash: String,
    pub short_hash: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct GitLogFile {
    pub status: String,
    pub path: String,
    diff_path: String,
    pub diff: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
enum GitOperation {
    CreateBranch(String),
    SwitchBranch(String),
    Push,
    Pull,
}

#[derive(Debug, Clone)]
pub enum PendingFileAction {
    New,
    Open(PathBuf),
    Quit,
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
    SubMenuItem { label: "Run",           shortcut: "Ctrl+F9" },
    SubMenuItem { label: "Compile",       shortcut: "F9"      },
    SubMenuItem { label: "",             shortcut: ""     },
    SubMenuItem { label: "Toggle Breakpoint", shortcut: "F8" },
    SubMenuItem { label: "Debug",         shortcut: "Shift+F8" },
    SubMenuItem { label: "",             shortcut: ""     },
    SubMenuItem { label: "Parameters...", shortcut: ""    },
];

const GIT_MENU: &[SubMenuItem] = &[
    SubMenuItem { label: "Review Changes...", shortcut: "" },
    SubMenuItem { label: "Branches...",       shortcut: "" },
    SubMenuItem { label: "Log...",            shortcut: "" },
    SubMenuItem { label: "",                  shortcut: "" },
    SubMenuItem { label: "Push",              shortcut: "" },
    SubMenuItem { label: "Pull",              shortcut: "" },
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

/// Indexed by menu_index (0 = File … 7 = Help).
pub const SUBMENUS: &[&[SubMenuItem]] = &[
    FILE_MENU, EDIT_MENU, SEARCH_MENU, RUN_MENU, GIT_MENU, OPTIONS_MENU, WINDOW_MENU, HELP_MENU,
];

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub editor: Editor,
    pub buffers: Vec<Editor>,
    pub focus: FocusArea,
    pub menu_index: usize,
    /// Whether a dropdown submenu is currently open.
    pub submenu_open: bool,
    /// Selected item index within the open submenu.
    pub submenu_index: usize,
    pub show_tree: bool,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub tree_scroll: usize,
    collapsed_dirs: HashSet<PathBuf>,
    mouse_selecting: bool,
    pub preserve_editor_scroll: bool,
    pub dialog: Option<Dialog>,
    pub completion: CompletionState,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub breakpoints: HashMap<PathBuf, HashSet<usize>>,
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
    settings: ProjectSettings,
    /// Set to `true` after a file-open that changed the language; the event
    /// loop will restart the LSP and clear this flag.
    pending_lsp_restart: bool,
    pending_lsp_document: Option<String>,
    pending_debugger: bool,
    pending_run: bool,
    pending_docker: Option<(String, Vec<String>)>,
    pending_docker_approval: bool,
    pending_git_operation: Option<GitOperation>,
    enhanced_keyboard: bool,
    pub git_changed_lines: HashSet<usize>,
    git_last_refresh: tokio::time::Instant,
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
        let (mut server_cmd, lsp_launch_mode) =
            resolve_server_command(current_language, &root_str);
        let pending_docker = if lsp_launch_mode == "docker" {
            server_cmd.take().and_then(|argv| docker_image_from_argv(&argv).map(|image| (image, argv)))
        } else {
            None
        };

        let lsp = LspClient::new(root_path.clone(), server_cmd);
        let tree_entries = build_tree(&root_path);
        let settings = std::fs::read_to_string(root_path.join(".blueide.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(Self {
            editor,
            buffers: Vec::new(),
            focus: FocusArea::Editor,
            menu_index: 0,
            submenu_open: false,
            submenu_index: 0,
            show_tree: true,
            tree_entries,
            tree_selected: 0,
            tree_scroll: 0,
            collapsed_dirs: HashSet::new(),
            mouse_selecting: false,
            preserve_editor_scroll: false,
            dialog: None,
            completion: CompletionState::Hidden,
            diagnostics: HashMap::new(),
            breakpoints: HashMap::new(),
            lsp_available: false,
            status_msg: None,
            clipboard: String::new(),
            last_needle: String::new(),
            current_language,
            lsp_launch_mode,
            settings,
            pending_lsp_restart: false,
            pending_lsp_document: None,
            pending_debugger: false,
            pending_run: false,
            pending_docker,
            pending_docker_approval: false,
            pending_git_operation: None,
            enhanced_keyboard: false,
            git_changed_lines: HashSet::new(),
            git_last_refresh: tokio::time::Instant::now(),
            lsp,
            root_path,
        })
    }

    // ------------------------------------------------------------------
    // Main run loop
    // ------------------------------------------------------------------

    pub async fn run(&mut self) -> Result<()> {
        // Set up terminal.
        let enhanced_keys = crossterm::terminal::supports_keyboard_enhancement()
            .unwrap_or(false);
        self.enhanced_keyboard = enhanced_keys;
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
        )?;
        if enhanced_keys {
            crossterm::execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
            )?;
        } else {
            self.status_msg = Some("Terminal cannot distinguish Shift+Enter; use Ctrl+J to insert below.".into());
        }
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.hide_cursor()?;

        // Ask before a Docker fallback can pull or run a language server.
        if let Some((image, _)) = &self.pending_docker {
            self.dialog = Some(Dialog::DockerApproval { image: image.clone() });
        } else {
            if let Err(error) = self.start_lsp_for_current_document().await {
                self.status_msg = Some(format!("LSP startup failed: {error:#}"));
            }
        }

        let result = self.event_loop(&mut terminal).await;

        // Restore terminal.
        self.lsp.stop().await;
        crossterm::terminal::disable_raw_mode()?;
        if enhanced_keys {
            crossterm::execute!(
                terminal.backend_mut(),
                PopKeyboardEnhancementFlags,
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture,
            )?;
        } else {
            crossterm::execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture,
            )?;
        }
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
            } else if let Some(old_uri) = self.pending_lsp_document.take() {
                self.switch_lsp_document(&old_uri).await;
            }

            // Drain any pending LSP events first (non-blocking).
            self.drain_lsp_events();

            // Wait for terminal event or timeout.
            if event::poll(tick)? {
                match event::read()? {
                    Event::Key(key) => {
                        let dialog_was_open = self.dialog.is_some();
                        if self.handle_key(key).await? {
                            break; // quit
                        }
                        if dialog_was_open && self.dialog.is_none() {
                            terminal.clear()?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let dialog_was_open = self.dialog.is_some();
                        if self.handle_mouse(mouse).await? {
                            break;
                        }
                        if dialog_was_open && self.dialog.is_none() {
                            terminal.clear()?;
                        }
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            if self.pending_debugger {
                self.pending_debugger = false;
                self.run_debugger(terminal).await?;
            }
            if self.pending_run {
                self.pending_run = false;
                self.run_program(terminal).await?;
            }
            if self.pending_docker_approval {
                self.pending_docker_approval = false;
                self.dialog = None;
                self.status_msg = Some("Preparing Docker language server…".into());
                terminal.draw(|f| crate::ui::render(f, self))?;
                self.approve_docker_lsp().await;
            }
            if let Some(operation) = self.pending_git_operation.take() {
                terminal.draw(|f| crate::ui::render(f, self))?;
                self.run_git_operation(operation).await;
            }
            self.autosave_idle_buffers();
            if self.git_last_refresh.elapsed() >= Duration::from_secs(1) {
                self.refresh_git_line_markers().await;
                self.git_last_refresh = tokio::time::Instant::now();
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
        self.preserve_editor_scroll = false;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            // ---- Quit ----
            KeyCode::Char('q') | KeyCode::Char('c') if ctrl => {
                if self.editor.modified {
                    self.dialog = Some(Dialog::ConfirmDiscard { action: PendingFileAction::Quit });
                } else {
                    return Ok(true);
                }
            }
            KeyCode::F(4) if alt => {
                if self.editor.modified {
                    self.dialog = Some(Dialog::ConfirmDiscard { action: PendingFileAction::Quit });
                } else {
                    return Ok(true);
                }
            }

            // Borland-style build/run pair: F9 compiles, Ctrl+F9 runs.
            KeyCode::F(9) if ctrl => self.pending_run = true,
            KeyCode::F(9) => self.compile_project().await,
            KeyCode::F(5) => {
                self.show_tree = !self.show_tree;
                self.focus = if self.show_tree { FocusArea::Tree } else { FocusArea::Editor };
            }
            KeyCode::F(6) => self.show_hover().await,
            KeyCode::Tab if ctrl => self.next_buffer(),
            KeyCode::BackTab if ctrl => self.previous_buffer(),
            KeyCode::PageDown if ctrl => self.next_buffer(),
            KeyCode::PageUp if ctrl => self.previous_buffer(),
            KeyCode::Char(c) if ctrl && ('1'..='9').contains(&c) => {
                self.select_tab((c as usize) - ('1' as usize));
            }
            KeyCode::F(8) if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.pending_debugger = true;
                self.status_msg = Some("Starting native debugger…".into());
            }
            KeyCode::F(8) => self.toggle_breakpoint(),

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
            KeyCode::Char('a') if ctrl => self.editor.select_all(),

            // ---- LSP: completion ----
            KeyCode::Char(' ') if ctrl => {
                self.trigger_completion().await;
            }

            // ---- LSP: go to definition ----
            KeyCode::F(12) => {
                self.go_to_definition().await;
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
                if let Some(selected) = self.editor.selected_text() {
                    self.clipboard = selected;
                    self.editor.delete_selection();
                } else {
                    self.clipboard = self.editor.cut_line();
                }
                self.notify_lsp_change().await;
            }
            KeyCode::Insert if ctrl => {
                self.clipboard = self.editor.selected_text().unwrap_or_else(|| self.editor.copy_line());
            }
            KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.editor.insert_str(&text);
                    self.notify_lsp_change().await;
                }
            }

            // ---- Cursor movement ----
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_up(); }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_down(); }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_left(); }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_right(); }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_home(); }
            KeyCode::End if key.modifiers.contains(KeyModifiers::SHIFT) => { self.editor.begin_selection(); self.editor.move_end(); }
            KeyCode::Up => { self.editor.clear_selection(); self.editor.move_up(); }
            KeyCode::Down => { self.editor.clear_selection(); self.editor.move_down(); }
            KeyCode::Left if ctrl => { self.editor.clear_selection(); self.editor.move_word_left(); }
            KeyCode::Right if ctrl => { self.editor.clear_selection(); self.editor.move_word_right(); }
            KeyCode::Left => { self.editor.clear_selection(); self.editor.move_left(); }
            KeyCode::Right => { self.editor.clear_selection(); self.editor.move_right(); }
            KeyCode::Home => { self.editor.clear_selection(); self.editor.move_home(); }
            KeyCode::End => { self.editor.clear_selection(); self.editor.move_end(); }
            KeyCode::PageUp => { self.editor.clear_selection(); self.editor.move_page_up(20); }
            KeyCode::PageDown => { self.editor.clear_selection(); self.editor.move_page_down(20); }

            // ---- Text input ----
            KeyCode::Char('j') if ctrl => {
                self.editor.insert_newline_below();
                self.notify_lsp_change().await;
            }
            KeyCode::Char('\n') => {
                self.editor.insert_newline_below();
                self.notify_lsp_change().await;
            }
            KeyCode::Char(c) => {
                if let Some(close) = match c {
                    '(' => Some(')'), '[' => Some(']'), '{' => Some('}'),
                    _ => None,
                } {
                    self.editor.insert_pair(c, close);
                } else if matches!(c, ')' | ']' | '}')
                    && self.editor.lines()[self.editor.cursor_row]
                        .chars()
                        .nth(self.editor.cursor_col) == Some(c)
                {
                    self.editor.move_right();
                } else {
                    self.editor.insert_char(c);
                }
                self.notify_lsp_change().await;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.editor.insert_newline_below();
                self.notify_lsp_change().await;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.insert_newline_below();
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
            (0, "Exit") => {
                if self.editor.modified {
                    self.dialog = Some(Dialog::ConfirmDiscard { action: PendingFileAction::Quit });
                } else {
                    return Ok(true);
                }
            }

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
            (3, "Run") => self.pending_run = true,
            (3, "Compile") => self.compile_project().await,
            (3, "Toggle Breakpoint") => self.toggle_breakpoint(),
            (3, "Debug") => self.pending_debugger = true,
            (3, "Parameters...") => {
                self.dialog = Some(Dialog::SettingsInput {
                    kind: SettingKind::Parameters,
                    input: self.settings.run_parameters.clone(),
                });
            }

            // ── Git ────────────────────────────────────────────────────
            (4, "Review Changes...") => self.open_git_review().await,
            (4, "Branches...") => self.open_git_branches().await,
            (4, "Log...") => self.open_git_log().await,
            (4, "Push") => {
                self.pending_git_operation = Some(GitOperation::Push);
                self.status_msg = Some("Pushing current branch to origin…".into());
            }
            (4, "Pull") => {
                self.pending_git_operation = Some(GitOperation::Pull);
                self.status_msg = Some("Pulling current branch (fast-forward only)…".into());
            }

            // ── Options ─────────────────────────────────────────────────
            (5, "Compiler...") => {
                if let Some(lang) = self.current_language {
                    self.dialog = Some(Dialog::SettingsInput {
                        kind: SettingKind::Compiler(lang.id.into()),
                        input: self.settings.compiler_overrides.get(lang.id).cloned().unwrap_or_default(),
                    });
                } else {
                    self.dialog = Some(Dialog::Message { text: "Open a source file to configure its compiler command.".into() });
                }
            }
            (5, "Environment...") => {
                let lsp_status = if self.lsp_available {
                    format!("LSP: active ({})", self.lsp_launch_mode)
                } else {
                    "LSP: inactive".into()
                };
                self.dialog = Some(Dialog::Message {
                    text: format!(
                        "Project: {}\nActive buffer: {}\nTheme: Turbo Pascal blue\n{lsp_status}",
                        self.root_path.display(),
                        self.editor.display_name(),
                    ),
                });
            }
            (5, "Save Settings") => {
                match serde_json::to_string_pretty(&self.settings)
                    .map_err(anyhow::Error::from)
                    .and_then(|json| std::fs::write(self.root_path.join(".blueide.json"), json).map_err(anyhow::Error::from))
                {
                    Ok(()) => self.status_msg = Some("Settings saved to .blueide.json.".into()),
                    Err(error) => self.dialog = Some(Dialog::Message { text: format!("Could not save settings: {error}") }),
                }
            }

            // ── Window ───────────────────────────────────────────────────
            (6, "Toggle File Browser") => {
                self.show_tree = !self.show_tree;
                if self.show_tree {
                    self.focus = FocusArea::Tree;
                }
            }
            (6, "Refresh Display") => {
                self.status_msg = Some("Display refreshed.".into());
            }
            (6, "Close") => {
                // Close current buffer (new empty file).
                self.new_file();
            }

            // ── Help ─────────────────────────────────────────────────────
            (7, "Contents") | (7, "Index") | (7, "Topic Search") => {
                self.dialog = Some(Dialog::Help);
            }
            (7, "About...") => {
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
            KeyCode::Right => {
                if let Some(entry) = self.tree_entries.get(self.tree_selected) {
                    if entry.path.is_dir() && self.collapsed_dirs.remove(&entry.path) {
                        self.refresh_tree(Some(entry.path.clone()));
                    }
                }
            }
            KeyCode::Left => {
                if let Some(entry) = self.tree_entries.get(self.tree_selected) {
                    let path = entry.path.clone();
                    let directory = if path.is_dir() { path.clone() } else { path.parent().unwrap_or(&path).to_path_buf() };
                    if directory != self.root_path && !self.collapsed_dirs.contains(&directory) {
                        self.collapsed_dirs.insert(directory.clone());
                        self.refresh_tree(Some(directory));
                    } else if let Some(parent) = directory.parent() {
                        if parent != self.root_path {
                            let parent = parent.to_path_buf();
                            self.collapsed_dirs.insert(parent.clone());
                            self.refresh_tree(Some(parent));
                        }
                    }
                }
            }
            KeyCode::Enter => {
                self.activate_tree_entry(self.tree_selected);
            }
            _ => {}
        }
        Ok(false)
    }

    fn activate_tree_entry(&mut self, index: usize) {
        let Some(entry) = self.tree_entries.get(index) else { return; };
        let path = entry.path.clone();
        if path.is_file() {
            self.open_file_path(path);
            self.focus = FocusArea::Editor;
        } else if path.is_dir() {
            if !self.collapsed_dirs.remove(&path) {
                self.collapsed_dirs.insert(path.clone());
            }
            self.refresh_tree(Some(path));
        }
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<bool> {
        if let Some(dialog) = self.dialog.clone() {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
                if let Some(Dialog::GitChanges { changes, selected, .. }) = &mut self.dialog {
                    if let Some(index) = crate::ui::git_review_item_at(
                        &dialog,
                        Rect::new(0, 0, width, height),
                        mouse.column,
                        mouse.row,
                    ) {
                        *selected = index.min(changes.len().saturating_sub(1));
                        return Ok(false);
                    }
                }
                if let Some(Dialog::GitBranches { branches, selected, .. }) = &mut self.dialog {
                    if let Some(index) = crate::ui::git_branch_item_at(
                        &dialog,
                        Rect::new(0, 0, width, height),
                        mouse.column,
                        mouse.row,
                    ) {
                        *selected = index.min(branches.len().saturating_sub(1));
                        return Ok(false);
                    }
                }
                if let Some(index) = crate::ui::git_log_file_at(
                    &dialog,
                    Rect::new(0, 0, width, height),
                    mouse.column,
                    mouse.row,
                ) {
                    if let Some(Dialog::GitLog { selected_file, file_focus, .. }) = &mut self.dialog {
                        *selected_file = index;
                        *file_focus = true;
                        return Ok(false);
                    }
                }
                if let Some(index) = crate::ui::git_log_item_at(
                    &dialog,
                    Rect::new(0, 0, width, height),
                    mouse.column,
                    mouse.row,
                ) {
                    let entry = if let Some(Dialog::GitLog { entries, selected, selected_file, file_focus, expanded_file, scroll, .. }) = &mut self.dialog {
                        *selected = index.min(entries.len().saturating_sub(1));
                        *selected_file = 0;
                        *file_focus = false;
                        *expanded_file = None;
                        *scroll = 0;
                        entries.get(*selected).cloned()
                    } else { None };
                    if let Some(entry) = entry {
                        match self.load_git_commit_details(&entry).await {
                            Ok((new_message, new_files)) => {
                                if let Some(Dialog::GitLog { message, files, .. }) = &mut self.dialog {
                                    *message = new_message;
                                    *files = new_files;
                                }
                            }
                            Err(error) => self.status_msg = Some(format!("Could not load commit: {error:#}")),
                        }
                    }
                    return Ok(false);
                }
                if let Some(button) = crate::ui::dialog_button_at(
                    &dialog,
                    Rect::new(0, 0, width, height),
                    mouse.column,
                    mouse.row,
                ) {
                    let key = match dialog {
                        Dialog::DockerApproval { .. } if button == 0 => KeyCode::Char('y'),
                        Dialog::DockerApproval { .. } => KeyCode::Esc,
                        Dialog::ConfirmDiscard { .. } if button == 0 => KeyCode::Char('y'),
                        Dialog::ConfirmDiscard { .. } => KeyCode::Char('n'),
                        Dialog::BuildOutput { .. } => KeyCode::Esc,
                        Dialog::Message { .. } => KeyCode::Enter,
                        _ if button == 0 => KeyCode::Enter,
                        _ => KeyCode::Esc,
                    };
                    return self.handle_dialog_key(KeyEvent::new(key, KeyModifiers::NONE)).await;
                }
            }
            if let Some(Dialog::GitChanges { scroll, .. }) = &mut self.dialog {
                match mouse.kind {
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(3),
                    MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(3),
                    _ => {}
                }
                return Ok(false);
            }
            if let Some(Dialog::GitLog { scroll, .. }) = &mut self.dialog {
                match mouse.kind {
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(3),
                    MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(3),
                    _ => {}
                }
                return Ok(false);
            }
            if let Some(Dialog::Hover { ref mut scroll, .. }) = self.dialog {
                match mouse.kind {
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(3),
                    MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(3),
                    _ => {}
                }
            }
            return Ok(false);
        }
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        let status_row = height.saturating_sub(1);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_selecting = false;
                if mouse.row == 0 {
                    if let Some(menu) = self.menu_at_x(mouse.column as usize) {
                        self.menu_index = menu;
                        self.focus = FocusArea::Menu;
                        self.open_submenu();
                    }
                } else if self.submenu_open && self.submenu_hit(mouse.column as usize, mouse.row as usize, width as usize) {
                    let item = mouse.row.saturating_sub(2) as usize;
                    if item < SUBMENUS[self.menu_index].len() {
                        self.submenu_open = false;
                        let quit = self.activate_submenu_item(self.menu_index, item).await?;
                        if self.focus == FocusArea::Menu { self.focus = FocusArea::Editor; }
                        return Ok(quit);
                    }
                } else if mouse.row == 1 {
                    if let Some(tab) = self.tab_at_x(mouse.column as usize) {
                        self.select_tab(tab);
                        self.focus = FocusArea::Editor;
                    }
                } else if self.show_tree && mouse.column < 24 && mouse.row >= 3 && mouse.row < status_row {
                    let visible_index = mouse.row.saturating_sub(3) as usize;
                    let index = self.tree_scroll + visible_index;
                    if index < self.tree_entries.len() {
                        self.tree_selected = index;
                        self.focus = FocusArea::Tree;
                        self.activate_tree_entry(index);
                    }
                } else if mouse.row >= 2 && mouse.row < status_row {
                    self.preserve_editor_scroll = false;
                    let editor_x = if self.show_tree { 24 } else { 0 };
                    if mouse.column as usize >= editor_x {
                        let row = self.editor.scroll_row + mouse.row.saturating_sub(2) as usize;
                        if row < self.editor.line_count() {
                            self.focus = FocusArea::Editor;
                            let column = mouse.column as usize - editor_x;
                            if column < 6 {
                                self.editor.cursor_row = row;
                                self.editor.clear_selection();
                                self.toggle_breakpoint();
                            } else {
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    self.editor.begin_selection();
                                } else {
                                    self.editor.clear_selection();
                                    self.editor.cursor_row = row;
                                    self.editor.cursor_col = self.editor.scroll_col + column - 6;
                                    self.editor.begin_selection();
                                }
                                self.editor.cursor_row = row;
                                self.editor.cursor_col = (self.editor.scroll_col + column - 6)
                                    .min(self.editor.lines()[row].chars().count());
                                self.mouse_selecting = true;
                            }
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse_selecting => {
                self.preserve_editor_scroll = false;
                self.place_mouse_cursor(mouse.column, mouse.row, width, status_row);
            }
            MouseEventKind::Up(MouseButton::Left) => self.mouse_selecting = false,
            MouseEventKind::ScrollUp => {
                if mouse.row >= 2 && mouse.row < status_row && self.show_tree && mouse.column < 24 {
                    self.focus = FocusArea::Tree;
                    self.tree_selected = self.tree_selected.saturating_sub(3);
                } else if mouse.row >= 2 && mouse.row < status_row {
                    self.focus = FocusArea::Editor;
                    self.preserve_editor_scroll = true;
                    self.editor.scroll_row = self.editor.scroll_row.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if mouse.row >= 2 && mouse.row < status_row && self.show_tree && mouse.column < 24 {
                    self.focus = FocusArea::Tree;
                    self.tree_selected = (self.tree_selected + 3).min(self.tree_entries.len().saturating_sub(1));
                } else if mouse.row >= 2 && mouse.row < status_row {
                    self.focus = FocusArea::Editor;
                    self.preserve_editor_scroll = true;
                    let visible = (height as usize).saturating_sub(3).max(1);
                    self.editor.scroll_row = (self.editor.scroll_row + 3)
                        .min(self.editor.line_count().saturating_sub(visible));
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn place_mouse_cursor(&mut self, column: u16, row: u16, _width: u16, status_row: u16) {
        if row < 2 || row >= status_row { return; }
        let editor_x = if self.show_tree { 24usize } else { 0 };
        if (column as usize) < editor_x + 6 { return; }
        let target_row = self.editor.scroll_row + row.saturating_sub(2) as usize;
        if target_row >= self.editor.line_count() { return; }
        self.editor.cursor_row = target_row;
        self.editor.cursor_col = (self.editor.scroll_col + column as usize - editor_x - 6)
            .min(self.editor.lines()[target_row].chars().count());
    }

    fn tab_at_x(&self, x: usize) -> Option<usize> {
        let mut left = 1usize;
        let active_width = self.editor.display_name().chars().count() + 2;
        if (left..left + active_width).contains(&x) { return Some(0); }
        left += active_width;
        for (index, buffer) in self.buffers.iter().enumerate() {
            let tab_width = buffer.display_name().chars().count() + 2;
            if (left..left + tab_width).contains(&x) { return Some(index + 1); }
            left += tab_width;
        }
        None
    }

    fn menu_at_x(&self, x: usize) -> Option<usize> {
        let mut left = 1usize;
        for (index, label) in ["File", "Edit", "Search", "Run", "Options", "Window", "Help"].iter().enumerate() {
            let width = label.chars().count();
            if (left..left + width).contains(&x) { return Some(index); }
            left += width + 2;
        }
        None
    }

    fn submenu_hit(&self, x: usize, y: usize, frame_width: usize) -> bool {
        let items = SUBMENUS[self.menu_index];
        if y < 2 || y - 2 >= items.len() { return false; }
        let max_label = items.iter().filter(|i| !i.is_separator()).map(|i| i.label.len()).max().unwrap_or(8);
        let max_shortcut = items.iter().filter(|i| !i.is_separator()).map(|i| i.shortcut.len()).max().unwrap_or(0);
        let inner_width = (1 + max_label + 2 + max_shortcut + 1).max(16);
        let popup_width = inner_width + 2;
        let x_raw = menu_position(self.menu_index);
        let popup_x = x_raw.min(frame_width.saturating_sub(popup_width));
        x >= popup_x && x < popup_x + popup_width
    }

    fn refresh_tree(&mut self, select_path: Option<PathBuf>) {
        let all_entries = build_tree(&self.root_path);
        self.tree_entries = all_entries
            .into_iter()
            .filter(|entry| {
                let mut parent = entry.path.parent();
                while let Some(path) = parent {
                    if self.collapsed_dirs.contains(path) { return false; }
                    if path == self.root_path { break; }
                    parent = path.parent();
                }
                true
            })
            .map(|mut entry| {
                if entry.path.is_dir() {
                    let marker = if self.collapsed_dirs.contains(&entry.path) { "▸ " } else { "▾ " };
                    if let Some(index) = entry.display.find('▾').or_else(|| entry.display.find('▸')) {
                        entry.display.replace_range(index..index + '▾'.len_utf8(), marker);
                    }
                }
                entry
            })
            .collect();
        if let Some(path) = select_path {
            if let Some(index) = self.tree_entries.iter().position(|entry| entry.path == path) {
                self.tree_selected = index;
                return;
            }
        }
        self.tree_selected = self.tree_selected.min(self.tree_entries.len().saturating_sub(1));
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
            Some(Dialog::ConfirmDiscard { action }) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => match action {
                    PendingFileAction::Quit => return Ok(true),
                    action => self.apply_file_action(action),
                },
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {}
                _ => self.dialog = Some(Dialog::ConfirmDiscard { action }),
            },
            Some(Dialog::DockerApproval { image }) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter | KeyCode::Char('\n') => {
                    self.dialog = None;
                    self.pending_docker_approval = true;
                    self.status_msg = Some("Preparing Docker language server…".into());
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.pending_docker = None;
                    self.lsp_launch_mode = "none";
                    self.status_msg = Some(format!("Docker LSP declined ({image})."));
                }
                _ => self.dialog = Some(Dialog::DockerApproval { image }),
            },
            Some(Dialog::GitChanges { mut changes, mut selected, mut scroll }) => {
                match key.code {
                    KeyCode::Esc => return Ok(false),
                    KeyCode::Up => { selected = selected.saturating_sub(1); scroll = 0; }
                    KeyCode::Down => { selected = (selected + 1).min(changes.len().saturating_sub(1)); scroll = 0; }
                    KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                    KeyCode::PageDown => scroll = scroll.saturating_add(10),
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        let output = Command::new("git")
                            .args(["add", "-A"])
                            .current_dir(&self.root_path)
                            .output()
                            .await?;
                        if output.status.success() {
                            self.status_msg = Some("All changes staged.".into());
                            changes = self.load_git_changes().await.unwrap_or_default();
                            selected = selected.min(changes.len().saturating_sub(1));
                        } else {
                            self.status_msg = Some(format!("Could not stage all changes: {}", String::from_utf8_lossy(&output.stderr).trim()));
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        match self.load_git_changes().await {
                            Ok(updated) => changes = updated,
                            Err(error) => self.status_msg = Some(format!("Git refresh failed: {error:#}")),
                        }
                    }
                    KeyCode::Char(' ') => {
                        if let Some(change) = changes.get(selected).cloned() {
                            match self.stage_git_change(&change).await {
                                Ok(()) => {
                                    self.status_msg = Some(format!("Updated staging: {}", change.path.display()));
                                    changes = self.load_git_changes().await.unwrap_or_default();
                                    selected = selected.min(changes.len().saturating_sub(1));
                                }
                                Err(error) => self.status_msg = Some(format!("Git staging failed: {error:#}")),
                            }
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        if changes.iter().any(|change| change.index_status != ' ' && change.index_status != '?') {
                            self.dialog = Some(Dialog::GitCommit { changes, selected, scroll, message: String::new() });
                            return Ok(false);
                        }
                        self.status_msg = Some("Stage at least one change before committing.".into());
                    }
                    KeyCode::Enter => {
                        if let Some(change) = changes.get(selected) {
                            self.open_file_path(self.root_path.join(&change.path));
                            return Ok(false);
                        }
                    }
                    _ => {}
                }
                self.dialog = Some(Dialog::GitChanges { changes, selected, scroll });
            }
            Some(Dialog::GitCommit { changes, selected, scroll, mut message }) => {
                match key.code {
                    KeyCode::Esc => self.dialog = Some(Dialog::GitChanges { changes, selected, scroll }),
                    KeyCode::Enter if !message.trim().is_empty() => {
                        match self.commit_staged_changes(message.trim()).await {
                            Ok(()) => {}
                            Err(error) => {
                                self.status_msg = Some(format!("Commit failed: {error:#}"));
                                self.dialog = Some(Dialog::GitCommit { changes, selected, scroll, message });
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        message.pop();
                        self.dialog = Some(Dialog::GitCommit { changes, selected, scroll, message });
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) => {
                        message.push(c);
                        self.dialog = Some(Dialog::GitCommit { changes, selected, scroll, message });
                    }
                    _ => self.dialog = Some(Dialog::GitCommit { changes, selected, scroll, message }),
                }
            }
            Some(Dialog::GitBranches { branches, current, mut selected }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                        return Ok(false);
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(branches.len().saturating_sub(1));
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                        return Ok(false);
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.dialog = Some(Dialog::GitNewBranch { branches, current, selected, name: String::new() });
                        return Ok(false);
                    }
                    KeyCode::Enter => {
                        if let Some(branch) = branches.get(selected) {
                            if branch != &current {
                                self.pending_git_operation = Some(GitOperation::SwitchBranch(branch.clone()));
                                self.status_msg = Some(format!("Switching to branch {branch}…"));
                            }
                        }
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                    }
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        self.pending_git_operation = Some(GitOperation::Push);
                        self.status_msg = Some("Pushing current branch to origin…".into());
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        self.pending_git_operation = Some(GitOperation::Pull);
                        self.status_msg = Some("Pulling current branch (fast-forward only)…".into());
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        match self.load_git_branches().await {
                            Ok((updated, active)) => {
                                selected = updated.iter().position(|branch| branch == &active).unwrap_or(0);
                                self.dialog = Some(Dialog::GitBranches { branches: updated, current: active, selected });
                            }
                            Err(error) => self.status_msg = Some(format!("Git branch refresh failed: {error:#}")),
                        }
                    }
                    _ => self.dialog = Some(Dialog::GitBranches { branches, current, selected }),
                }
            }
            Some(Dialog::GitNewBranch { branches, current, selected, mut name }) => {
                match key.code {
                    KeyCode::Esc => self.dialog = Some(Dialog::GitBranches { branches, current, selected }),
                    KeyCode::Enter if !name.trim().is_empty() => {
                        let branch_name = name.trim().to_owned();
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                        self.pending_git_operation = Some(GitOperation::CreateBranch(branch_name.clone()));
                        self.status_msg = Some(format!("Creating branch {branch_name}…"));
                    }
                    KeyCode::Backspace => {
                        name.pop();
                        self.dialog = Some(Dialog::GitNewBranch { branches, current, selected, name });
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) => {
                        name.push(c);
                        self.dialog = Some(Dialog::GitNewBranch { branches, current, selected, name });
                    }
                    _ => self.dialog = Some(Dialog::GitNewBranch { branches, current, selected, name }),
                }
            }
            Some(Dialog::GitLog {
                mut entries,
                mut selected,
                mut message,
                mut files,
                mut selected_file,
                mut file_focus,
                mut expanded_file,
                mut scroll,
            }) => {
                match key.code {
                    KeyCode::Esc => return Ok(false),
                    KeyCode::Left => { file_focus = false; expanded_file = None; scroll = 0; }
                    KeyCode::Right if !files.is_empty() => file_focus = true,
                    KeyCode::Tab if !files.is_empty() => file_focus = !file_focus,
                    KeyCode::Up if file_focus => {
                        selected_file = selected_file.saturating_sub(1);
                        expanded_file = None;
                        scroll = 0;
                    }
                    KeyCode::Down if file_focus => {
                        selected_file = (selected_file + 1).min(files.len().saturating_sub(1));
                        expanded_file = None;
                        scroll = 0;
                    }
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        selected_file = 0;
                        expanded_file = None;
                        scroll = 0;
                        if let Some(entry) = entries.get(selected).cloned() {
                            match self.load_git_commit_details(&entry).await {
                                Ok((new_message, new_files)) => { message = new_message; files = new_files; }
                                Err(error) => self.status_msg = Some(format!("Could not load commit details: {error:#}")),
                            }
                        }
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(entries.len().saturating_sub(1));
                        selected_file = 0;
                        expanded_file = None;
                        scroll = 0;
                        if let Some(entry) = entries.get(selected).cloned() {
                            match self.load_git_commit_details(&entry).await {
                                Ok((new_message, new_files)) => { message = new_message; files = new_files; }
                                Err(error) => self.status_msg = Some(format!("Could not load commit details: {error:#}")),
                            }
                        }
                    }
                    KeyCode::Enter if file_focus => {
                        if expanded_file == Some(selected_file) {
                            expanded_file = None;
                            scroll = 0;
                        } else if let (Some(entry), Some(file)) = (entries.get(selected), files.get(selected_file)) {
                            match self.load_git_file_diff(entry, file).await {
                                Ok(diff) => {
                                    if let Some(file) = files.get_mut(selected_file) { file.diff = Some(diff); }
                                    expanded_file = Some(selected_file);
                                    scroll = 0;
                                }
                                Err(error) => self.status_msg = Some(format!("Could not load file diff: {error:#}")),
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if !files.is_empty() { file_focus = true; }
                    }
                    KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                    KeyCode::PageDown => scroll = scroll.saturating_add(10),
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        match self.load_git_log_entries().await {
                            Ok(updated) => {
                                entries = updated;
                                selected = selected.min(entries.len().saturating_sub(1));
                                selected_file = 0;
                                expanded_file = None;
                                scroll = 0;
                                if let Some(entry) = entries.get(selected).cloned() {
                                    match self.load_git_commit_details(&entry).await {
                                        Ok((new_message, new_files)) => { message = new_message; files = new_files; }
                                        Err(error) => self.status_msg = Some(format!("Could not load commit details: {error:#}")),
                                    }
                                }
                            }
                            Err(error) => self.status_msg = Some(format!("Git log refresh failed: {error:#}")),
                        }
                    }
                    _ => {
                        self.dialog = Some(Dialog::GitLog { entries, selected, message, files, selected_file, file_focus, expanded_file, scroll });
                        return Ok(false);
                    }
                }
                self.dialog = Some(Dialog::GitLog { entries, selected, message, files, selected_file, file_focus, expanded_file, scroll });
            }
            Some(Dialog::BuildOutput { title, lines, diagnostics, mut selected }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        self.dialog = Some(Dialog::BuildOutput { title, lines, diagnostics, selected });
                    }
                    KeyCode::Down => {
                        let max = if diagnostics.is_empty() { lines.len().saturating_sub(1) } else { diagnostics.len() - 1 };
                        selected = (selected + 1).min(max);
                        self.dialog = Some(Dialog::BuildOutput { title, lines, diagnostics, selected });
                    }
                    KeyCode::PageUp => {
                        selected = selected.saturating_sub(8);
                        self.dialog = Some(Dialog::BuildOutput { title, lines, diagnostics, selected });
                    }
                    KeyCode::PageDown => {
                        let max = if diagnostics.is_empty() { lines.len().saturating_sub(1) } else { diagnostics.len() - 1 };
                        selected = (selected + 8).min(max);
                        self.dialog = Some(Dialog::BuildOutput { title, lines, diagnostics, selected });
                    }
                    KeyCode::Enter if !diagnostics.is_empty() => {
                        let diagnostic = &diagnostics[selected.min(diagnostics.len() - 1)];
                        let path = diagnostic.path.clone();
                        let row = diagnostic.row;
                        let column = diagnostic.column;
                        if self.editor.file_path.as_ref() != Some(&path) {
                            self.open_file_path(path.clone());
                        }
                        if self.editor.file_path.as_ref() == Some(&path) {
                            self.editor.goto_line(row);
                            self.editor.cursor_col = column.min(self.editor.lines()[self.editor.cursor_row].chars().count());
                        }
                    }
                    _ => self.dialog = Some(Dialog::BuildOutput { title, lines, diagnostics, selected }),
                }
            }
            Some(Dialog::SettingsInput { kind, mut input }) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    match kind {
                        SettingKind::Parameters => self.settings.run_parameters = input,
                        SettingKind::Compiler(language) => {
                            if input.trim().is_empty() {
                                self.settings.compiler_overrides.remove(&language);
                            } else {
                                self.settings.compiler_overrides.insert(language, input.trim().into());
                            }
                        }
                    }
                    self.status_msg = Some("Setting updated. Choose Options → Save Settings to persist it.".into());
                }
                KeyCode::Backspace => {
                    input.pop();
                    self.dialog = Some(Dialog::SettingsInput { kind, input });
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    self.dialog = Some(Dialog::SettingsInput { kind, input });
                }
                _ => self.dialog = Some(Dialog::SettingsInput { kind, input }),
            },
            Some(Dialog::SaveAs { mut input }) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        let path = PathBuf::from(&input);
                        let old_uri = self.editor.uri();
                        let old_language = self.current_language.map(|l| l.id);
                        match self.editor.save_as(path) {
                            Ok(_) => {
                                self.current_language = self
                                    .editor
                                    .file_path
                                    .as_deref()
                                    .and_then(detect_language);
                                if old_language != self.current_language.map(|l| l.id) {
                                    self.pending_lsp_restart = true;
                                } else {
                                    self.pending_lsp_document = Some(old_uri);
                                }
                                self.status_msg = Some("Saved.".into());
                                self.refresh_tree(None);
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
            Some(Dialog::Hover { text, mut scroll }) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => {}
                    KeyCode::Up => scroll = scroll.saturating_sub(1),
                    KeyCode::Down => scroll = scroll.saturating_add(1),
                    KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                    KeyCode::PageDown => scroll = scroll.saturating_add(10),
                    KeyCode::Home => scroll = 0,
                    KeyCode::End => scroll = u16::MAX,
                    _ => {}
                }
                if !matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.dialog = Some(Dialog::Hover { text, scroll });
                }
            }
            Some(Dialog::Message { .. }) => {
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

    fn autosave_idle_buffers(&mut self) {
        let mut saved = Vec::new();
        let mut failures = Vec::new();

        for editor in std::iter::once(&mut self.editor).chain(self.buffers.iter_mut()) {
            if !editor.autosave_due(AUTOSAVE_IDLE) {
                continue;
            }
            let name = editor.display_name();
            match editor.save() {
                Ok(()) => saved.push(name),
                Err(error) => failures.push(format!("{name}: {error}")),
            }
        }

        if !failures.is_empty() {
            self.status_msg = Some(format!("Autosave failed — {}", failures.join("; ")));
        } else if !saved.is_empty() {
            self.status_msg = Some(format!("Autosaved {}", saved.join(", ")));
        }
    }

    fn new_file(&mut self) {
        if self.editor.modified {
            self.dialog = Some(Dialog::ConfirmDiscard {
                action: PendingFileAction::New,
            });
        } else {
            self.apply_file_action(PendingFileAction::New);
        }
    }

    fn open_file_path(&mut self, path: PathBuf) {
        if self.editor.modified {
            self.dialog = Some(Dialog::ConfirmDiscard {
                action: PendingFileAction::Open(path),
            });
            return;
        }
        self.load_file_path(path);
    }

    fn apply_file_action(&mut self, action: PendingFileAction) {
        match action {
            PendingFileAction::New => {
                let old_uri = self.editor.uri();
                if self.editor.file_path.is_some() {
                    self.buffers.push(self.editor.clone());
                }
                self.editor = Editor::default();
                self.current_language = None;
                self.pending_lsp_document = (!old_uri.is_empty()).then_some(old_uri);
                self.status_msg = Some("New file.".into());
            }
            PendingFileAction::Open(path) => self.load_file_path(path),
            PendingFileAction::Quit => {}
        }
    }

    fn load_file_path(&mut self, path: PathBuf) {
        if self.editor.file_path.as_ref() == Some(&path) {
            self.status_msg = Some("Already open.".into());
            return;
        }
        if let Some(index) = self.buffers.iter().position(|b| b.file_path.as_ref() == Some(&path)) {
            let next = self.buffers.remove(index);
            self.activate_editor(next);
            return;
        }
        match Editor::load_file(path.clone()) {
            Ok(ed) => {
                self.activate_editor(ed);
                self.status_msg = Some("File opened.".into());
            }
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    text: format!("Open failed: {e}"),
                });
            }
        }
    }

    fn activate_editor(&mut self, next: Editor) {
        let old_uri = self.editor.uri();
        let old_path = self.editor.file_path.clone();
        let new_path = next.file_path.clone();
        if old_path.is_some() && old_path != new_path {
            self.buffers.push(self.editor.clone());
        }
        self.buffers.retain(|buffer| buffer.file_path != new_path);
        self.editor = next;
        let new_lang = self.editor.file_path.as_deref().and_then(detect_language);
        let lang_changed = new_lang.map(|l| l.id) != self.current_language.map(|l| l.id);
        self.current_language = new_lang;
        if lang_changed {
            self.lsp_available = false;
            self.pending_lsp_restart = true;
            self.pending_lsp_document = None;
            let lang_name = new_lang.map(|l| l.name).unwrap_or("unknown");
            self.status_msg = Some(format!("Language: {lang_name} — restarting LSP…"));
        } else {
            self.pending_lsp_document = Some(old_uri);
        }
    }

    fn next_buffer(&mut self) {
        if self.buffers.is_empty() {
            self.status_msg = Some("No other open buffers.".into());
            return;
        }
        self.switch_buffer(0);
    }

    fn previous_buffer(&mut self) {
        if self.buffers.is_empty() {
            self.status_msg = Some("No other open buffers.".into());
            return;
        }
        self.switch_buffer(self.buffers.len() - 1);
    }

    /// Select an inactive-buffer slot. Slot zero is the first inactive tab.
    fn switch_buffer(&mut self, index: usize) {
        if index >= self.buffers.len() { return; }
        self.preserve_editor_scroll = false;
        let old_uri = self.editor.uri();
        let old_language = self.current_language.map(|l| l.id);
        let next = self.buffers.remove(index);
        let previous = std::mem::replace(&mut self.editor, next);
        if previous.file_path.is_some() || previous.modified {
            self.buffers.push(previous);
        }
        let next_language = self.editor.file_path.as_deref().and_then(detect_language);
        self.current_language = next_language;
        if old_language != next_language.map(|l| l.id) {
            self.pending_lsp_restart = true;
            self.pending_lsp_document = None;
        } else {
            self.pending_lsp_document = Some(old_uri);
        }
        self.status_msg = Some(format!("Buffer: {}", self.editor.display_name()));
    }

    fn select_tab(&mut self, tab: usize) {
        if tab == 0 { return; }
        self.switch_buffer(tab - 1);
    }

    fn toggle_breakpoint(&mut self) {
        let Some(path) = self.editor.file_path.as_ref() else {
            self.status_msg = Some("Save this buffer before setting a breakpoint.".into());
            return;
        };
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let rows = self.breakpoints.entry(path.clone()).or_default();
        if !rows.insert(self.editor.cursor_row) {
            rows.remove(&self.editor.cursor_row);
        }
        let state = if rows.contains(&self.editor.cursor_row) { "set" } else { "cleared" };
        self.status_msg = Some(format!("Breakpoint {state} at line {}.", self.editor.cursor_row + 1));
    }

    async fn run_debugger(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        if self.editor.modified {
            self.save_file();
            if self.editor.modified { return Ok(()); }
        }
        let root = self.root_path.canonicalize().unwrap_or_else(|_| self.root_path.clone());
        let Some(source) = self.editor.file_path.clone().or_else(|| first_buildable_file(&self.tree_entries)) else {
            self.dialog = Some(Dialog::Message { text: "No debuggable source file found in this project.".into() });
            return Ok(());
        };
        let source = source.canonicalize().unwrap_or(source);
        let source_language = detect_language(&source);
        let compiler_override = source_language
            .and_then(|lang| self.settings.compiler_overrides.get(lang.id))
            .map(String::as_str);
        let Some((mut compile_commands, binary)) = debugger_target(&source, &root) else {
            self.dialog = Some(Dialog::Message { text: "No native debugger recipe for this source type. GDB is supported for Rust, C, C++, and Free Pascal.".into() });
            return Ok(());
        };
        if let (Some(compiler), Some(command)) = (compiler_override, compile_commands.first_mut()) {
            let replacement = parse_arguments(compiler);
            if !replacement.is_empty() { command.splice(0..1, replacement); }
        }
        let mut output_lines = Vec::new();
        for args in compile_commands {
            output_lines.push(format!("$ {}", args.join(" ")));
            let mut command = Command::new(&args[0]);
            command.args(&args[1..]).current_dir(&root).kill_on_drop(true);
            match tokio::time::timeout(Duration::from_secs(120), command.output()).await {
                Ok(Ok(output)) => {
                    output_lines.extend(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned));
                    output_lines.extend(String::from_utf8_lossy(&output.stderr).lines().map(str::to_owned));
                    if !output.status.success() {
                        let diagnostics = parse_build_diagnostics(&output_lines, &root);
                        self.dialog = Some(Dialog::BuildOutput { title: "Debug build failed".into(), lines: output_lines, diagnostics, selected: 0 });
                        return Ok(());
                    }
                }
                Ok(Err(error)) => {
                    self.dialog = Some(Dialog::Message { text: format!("Could not build debug target: {error}") });
                    return Ok(());
                }
                Err(_) => {
                    self.dialog = Some(Dialog::Message { text: "Debug build timed out after 120 seconds.".into() });
                    return Ok(());
                }
            }
        }

        let mut debugger = Command::new("gdb");
        debugger.arg("--quiet").arg("--tui");
        for (path, points) in &self.breakpoints {
            for row in points {
                debugger.arg("-ex").arg(format!("break {}:{}", path.display(), row + 1));
            }
        }
        debugger.arg("-ex").arg("run").arg("--args").arg(&binary);
        debugger.args(parse_arguments(&self.settings.run_parameters)).current_dir(&root);

        terminal.show_cursor()?;
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::event::DisableMouseCapture)?;
        if self.enhanced_keyboard {
            crossterm::execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
        }
        let debugger_result = status_with_interrupt_guard(&mut debugger).await;
        if self.enhanced_keyboard {
            crossterm::execute!(
                terminal.backend_mut(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
            )?;
        }
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture)?;
        terminal.clear()?;
        terminal.hide_cursor()?;
        match debugger_result {
            Ok(status) => self.status_msg = Some(format!("Debugger exited with {status}. Use GDB commands for step, continue, print, and backtrace.")),
            Err(error) => self.dialog = Some(Dialog::Message { text: format!("Could not start GDB: {error}") }),
        }
        Ok(())
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
            if let Some(col_byte) = lines[row].find(needle) {
                self.editor.cursor_row = row;
                self.editor.cursor_col = lines[row][..col_byte].chars().count();
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

    async fn open_git_review(&mut self) {
        match self.load_git_changes().await {
            Ok(changes) if changes.is_empty() => {
                self.dialog = Some(Dialog::Message { text: "There are no Git changes to review.".into() });
            }
            Ok(changes) => self.dialog = Some(Dialog::GitChanges { changes, selected: 0, scroll: 0 }),
            Err(error) => self.dialog = Some(Dialog::Message { text: format!("Could not read Git changes: {error:#}") }),
        }
    }

    async fn load_git_branches(&self) -> Result<(Vec<String>, String)> {
        let branches_output = Command::new("git")
            .args(["branch", "--list", "--format=%(refname:short)"])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !branches_output.status.success() {
            let detail = String::from_utf8_lossy(&branches_output.stderr).trim().to_owned();
            anyhow::bail!("{}", if detail.is_empty() { "not inside a Git repository".to_owned() } else { detail });
        }
        let branches = String::from_utf8_lossy(&branches_output.stdout)
            .lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
        let current_output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.root_path)
            .output()
            .await?;
        let current = String::from_utf8_lossy(&current_output.stdout).trim().to_owned();
        Ok((branches, current))
    }

    async fn open_git_branches(&mut self) {
        match self.load_git_branches().await {
            Ok((branches, current)) => {
                let selected = branches.iter().position(|branch| branch == &current).unwrap_or(0);
                self.dialog = Some(Dialog::GitBranches { branches, current, selected });
            }
            Err(error) => self.dialog = Some(Dialog::Message { text: format!("Could not read Git branches: {error:#}") }),
        }
    }

    async fn open_git_log(&mut self) {
        match self.load_git_log_entries().await {
            Ok(entries) if entries.is_empty() => {
                self.dialog = Some(Dialog::Message { text: "This repository has no commits yet.".into() });
            }
            Ok(entries) => {
                let (message, files) = match self.load_git_commit_details(&entries[0]).await {
                    Ok(details) => details,
                    Err(error) => (format!("Could not load commit details: {error:#}"), Vec::new()),
                };
                self.dialog = Some(Dialog::GitLog {
                    entries,
                    selected: 0,
                    message,
                    files,
                    selected_file: 0,
                    file_focus: false,
                    expanded_file: None,
                    scroll: 0,
                });
            }
            Err(error) => self.dialog = Some(Dialog::Message { text: format!("Could not read Git log: {error:#}") }),
        }
    }

    async fn load_git_log_entries(&self) -> Result<Vec<GitCommitEntry>> {
        let output = Command::new("git")
            .args(["log", "-n", "100", "--date=short", "--format=%H%x00%h%x00%ad%x00%s%x00"])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            anyhow::bail!("{}", if detail.is_empty() { "not inside a Git repository".to_owned() } else { detail });
        }
        let fields: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .split('\0').map(|field| field.trim().to_owned()).collect();
        let entries = fields.chunks_exact(4).filter_map(|parts| {
            if parts[0].is_empty() { return None; }
            Some(GitCommitEntry {
                hash: parts[0].clone(),
                short_hash: parts[1].clone(),
                date: parts[2].clone(),
                subject: parts[3].clone(),
            })
        }).collect();
        Ok(entries)
    }

    async fn load_git_commit_details(&self, entry: &GitCommitEntry) -> Result<(String, Vec<GitLogFile>)> {
        let message = Command::new("git")
            .args(["show", "-s", "--format=%B", &entry.hash])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !message.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&message.stderr).trim());
        }
        let parent_info = Command::new("git")
            .args(["rev-list", "--parents", "-n", "1", &entry.hash])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !parent_info.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&parent_info.stderr).trim());
        }
        let first_parent = String::from_utf8_lossy(&parent_info.stdout)
            .split_whitespace().nth(1).map(str::to_owned);
        let mut files_command = Command::new("git");
        if let Some(parent) = first_parent {
            files_command.args(["diff", "--name-status", "--find-renames", &parent, &entry.hash]);
        } else {
            files_command.args(["diff-tree", "--root", "--no-commit-id", "--name-status", "--find-renames", "-r", &entry.hash]);
        }
        let files = files_command.current_dir(&self.root_path).output().await?;
        if !files.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&files.stderr).trim());
        }
        let message = String::from_utf8_lossy(&message.stdout).trim_end().to_owned();
        let files = String::from_utf8_lossy(&files.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let status = parts.next()?.trim();
                let first_path = parts.next()?.trim();
                if first_path.is_empty() { return None; }
                let (path, diff_path) = if status.starts_with('R') || status.starts_with('C') {
                    let new_path = parts.next().unwrap_or(first_path).trim();
                    (format!("{first_path} → {new_path}"), new_path.to_owned())
                } else {
                    (first_path.to_owned(), first_path.to_owned())
                };
                Some(GitLogFile { status: status.to_owned(), path, diff_path, diff: None })
            })
            .collect();
        Ok((message, files))
    }

    async fn load_git_file_diff(&self, entry: &GitCommitEntry, file: &GitLogFile) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["show", "--root", "--no-ext-diff", "--no-color", "--find-renames", "--format=", "--unified=3", &entry.hash, "--", &file.diff_path])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        let lines: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect();
        Ok(if lines.is_empty() { vec!["(No textual diff available)".into()] } else { lines })
    }

    async fn run_git_operation(&mut self, operation: GitOperation) {
        if matches!(&operation, GitOperation::SwitchBranch(_))
            && (self.editor.modified || self.buffers.iter().any(|buffer| buffer.modified))
        {
            self.status_msg = Some("Save or discard modified buffers before switching branches.".into());
            return;
        }
        let (mut command, description) = match &operation {
            GitOperation::CreateBranch(name) => {
                let mut command = Command::new("git");
                command.args(["switch", "-c", name]);
                (command, format!("create and switch to {name}"))
            }
            GitOperation::SwitchBranch(name) => {
                let mut command = Command::new("git");
                command.args(["switch", name]);
                (command, format!("switch to {name}"))
            }
            GitOperation::Push => {
                let mut command = Command::new("git");
                command.args(["push", "--set-upstream", "origin", "HEAD"]);
                (command, "push current branch".to_owned())
            }
            GitOperation::Pull => {
                let mut command = Command::new("git");
                command.args(["pull", "--ff-only"]);
                (command, "pull current branch".to_owned())
            }
        };
        let output = command.current_dir(&self.root_path).output().await;
        match output {
            Ok(output) if output.status.success() => {
                let response = String::from_utf8_lossy(&output.stdout);
                let detail = response.lines().last().filter(|line| !line.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Git {description} completed."));
                self.status_msg = Some(detail);
                if matches!(&operation, GitOperation::Pull | GitOperation::SwitchBranch(_)) {
                    if let Some(path) = self.editor.file_path.clone() {
                        if !self.editor.modified && path.exists() {
                            if let Ok(updated) = Editor::load_file(path) { self.activate_editor(updated); }
                        } else if self.editor.modified && matches!(&operation, GitOperation::Pull) {
                            self.status_msg = Some("Pull completed; the modified editor buffer was kept as-is.".into());
                        }
                    }
                    for buffer in &mut self.buffers {
                        if !buffer.modified {
                            if let Some(path) = buffer.file_path.clone() {
                                if path.exists() {
                                    if let Ok(updated) = Editor::load_file(path) { *buffer = updated; }
                                }
                            }
                        }
                    }
                }
                if let Ok((branches, current)) = self.load_git_branches().await {
                    let selected = branches.iter().position(|branch| branch == &current).unwrap_or(0);
                    if matches!(&self.dialog, Some(Dialog::GitBranches { .. })) {
                        self.dialog = Some(Dialog::GitBranches { branches, current, selected });
                    }
                }
                self.refresh_git_line_markers().await;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                self.status_msg = Some(format!("Git {description} failed: {detail}"));
            }
            Err(error) => self.status_msg = Some(format!("Could not run Git {description}: {error}")),
        }
    }

    async fn load_git_changes(&self) -> Result<Vec<GitChange>> {
        let output = Command::new("git")
            .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            anyhow::bail!("{}", if detail.is_empty() { "not inside a Git repository".to_owned() } else { detail });
        }

        let mut entries = Vec::new();
        let mut fields = output.stdout.split(|byte| *byte == 0).filter(|field| !field.is_empty());
        while let Some(field) = fields.next() {
            if field.len() < 4 { continue; }
            let index_status = field[0] as char;
            let worktree_status = field[1] as char;
            let path_bytes = &field[3..];
            if matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C') {
                // With -z Git reports the destination path first and the old
                // path second. Keep the destination for opening and staging.
                let _old_path = fields.next();
            }
            let path = PathBuf::from(String::from_utf8_lossy(path_bytes).into_owned());
            entries.push((path, index_status, worktree_status));
        }

        let mut changes = Vec::with_capacity(entries.len());
        for (path, index_status, worktree_status) in entries {
            let path_arg = path.to_string_lossy().into_owned();
            let mut diff = Command::new("git")
                .args(["diff", "--no-ext-diff", "--no-color", "--unified=3", "HEAD", "--", &path_arg])
                .current_dir(&self.root_path)
                .output()
                .await?;
            if !diff.status.success() || diff.stdout.is_empty() {
                if index_status == '?' {
                    diff = Command::new("git")
                        .args(["diff", "--no-index", "--no-ext-diff", "--no-color", "--unified=3", "--", "/dev/null", &path_arg])
                        .current_dir(&self.root_path)
                        .output()
                        .await?;
                }
            }
            let mut lines: Vec<String> = String::from_utf8_lossy(&diff.stdout)
                .lines()
                .map(str::to_owned)
                .collect();
            if lines.is_empty() { lines.push("(No textual diff available)".into()); }
            changes.push(GitChange { path, index_status, worktree_status, diff: lines });
        }
        Ok(changes)
    }

    async fn refresh_git_line_markers(&mut self) {
        let Some(path) = self.editor.file_path.as_ref() else {
            self.git_changed_lines.clear();
            return;
        };
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let canonical_root = self.root_path.canonicalize().unwrap_or_else(|_| self.root_path.clone());
        let relative = match canonical_path.strip_prefix(&canonical_root) {
            Ok(relative) => relative.to_path_buf(),
            Err(_) => {
                self.git_changed_lines.clear();
                return;
            }
        };
        let path_arg = relative.to_string_lossy().into_owned();
        let output = Command::new("git")
            .args(["diff", "--no-ext-diff", "--no-color", "--unified=0", "HEAD", "--", &path_arg])
            .current_dir(&self.root_path)
            .output()
            .await;
        let Ok(output) = output else { return; };
        self.git_changed_lines = if output.status.success() {
            changed_lines_from_diff(&String::from_utf8_lossy(&output.stdout))
        } else {
            HashSet::new()
        };

        if self.git_changed_lines.is_empty() && !path.exists() {
            return;
        }
        if self.git_changed_lines.is_empty() {
            let status = Command::new("git")
                .args(["status", "--porcelain", "--", &path_arg])
                .current_dir(&self.root_path)
                .output()
                .await;
            if let Ok(status) = status {
                if String::from_utf8_lossy(&status.stdout).starts_with("??") {
                    self.git_changed_lines = (0..self.editor.lines().len()).collect();
                }
            }
        }
    }

    async fn stage_git_change(&self, change: &GitChange) -> Result<()> {
        let path = change.path.to_string_lossy();
        let output = if change.index_status != ' ' && change.index_status != '?' {
            Command::new("git")
                .args(["restore", "--staged", "--", path.as_ref()])
                .current_dir(&self.root_path)
                .output()
                .await?
        } else {
            Command::new("git")
                .args(["add", "--", path.as_ref()])
                .current_dir(&self.root_path)
                .output()
                .await?
        };
        if output.status.success() { Ok(()) }
        else { anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim()) }
    }

    async fn commit_staged_changes(&mut self, message: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.root_path)
            .output()
            .await?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            anyhow::bail!("{}", if detail.is_empty() { String::from_utf8_lossy(&output.stdout).trim().to_string() } else { detail });
        }
        self.status_msg = Some(String::from_utf8_lossy(&output.stdout).lines().last().unwrap_or("Commit created.").to_owned());
        self.dialog = None;
        self.refresh_git_line_markers().await;
        Ok(())
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
        let (mut server_cmd, launch_mode) =
            resolve_server_command(self.current_language, &root_str);
        self.lsp_launch_mode = launch_mode;

        if launch_mode == "docker" {
            self.pending_docker = server_cmd
                .take()
                .and_then(|argv| docker_image_from_argv(&argv).map(|image| (image, argv)));
            self.lsp = LspClient::new(self.root_path.clone(), None);
            self.dialog = self.pending_docker.as_ref().map(|(image, _)| {
                Dialog::DockerApproval { image: image.clone() }
            });
            return;
        }
        self.pending_docker = None;

        // Create a brand-new client.
        self.lsp = LspClient::new(self.root_path.clone(), server_cmd);

        let lsp_ok = match self.lsp.start().await {
            Ok(available) => available,
            Err(error) => {
                self.lsp_available = false;
                self.status_msg = Some(format!("LSP startup failed: {error:#}"));
                return;
            }
        };
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

    async fn start_lsp_for_current_document(&mut self) -> Result<bool> {
        let lsp_ok = self.lsp.start().await?;
        self.lsp_available = lsp_ok;
        if lsp_ok && !self.editor.uri().is_empty() {
            let uri = self.editor.uri();
            let text = self.editor.text();
            let lang_id = self.current_language.map(|l| l.id).unwrap_or("plaintext");
            let _ = self.lsp.open_file(&uri, &text, lang_id).await;
        }
        Ok(lsp_ok)
    }

    async fn approve_docker_lsp(&mut self) {
        let Some((image, argv)) = self.pending_docker.take() else { return; };
        self.status_msg = Some(format!("Checking Docker image {image}…"));
        let image_exists = Command::new("docker")
            .args(["image", "inspect", image.as_str()])
            .output()
            .await
            .is_ok_and(|output| output.status.success());
        if !image_exists {
            self.status_msg = Some(format!("Pulling Docker image {image}…"));
            let pull = Command::new("docker")
                .args(["pull", image.as_str()])
                .output()
                .await;
            match pull {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    self.lsp_launch_mode = "none";
                    let message = if detail.is_empty() {
                        format!("Could not pull {image} (docker exited with {}).", output.status)
                    } else {
                        format!("Could not pull {image}: {detail}")
                    };
                    self.status_msg = Some(message.clone());
                    self.dialog = Some(Dialog::Message { text: message });
                    return;
                }
                Err(error) => {
                    self.lsp_launch_mode = "none";
                    let message = format!("Could not run docker pull: {error}");
                    self.status_msg = Some(message.clone());
                    self.dialog = Some(Dialog::Message { text: message });
                    return;
                }
            }
        }

        self.lsp = LspClient::new(self.root_path.clone(), Some(argv));
        match self.start_lsp_for_current_document().await {
            Ok(true) => {
                self.lsp_launch_mode = "docker";
                self.status_msg = Some(format!("Docker LSP started from {image}."));
            }
            Ok(false) => {
                self.lsp_launch_mode = "none";
                let message = format!("Docker LSP did not start from {image}.");
                self.status_msg = Some(message.clone());
                self.dialog = Some(Dialog::Message { text: message });
            }
            Err(error) => {
                self.lsp_available = false;
                self.lsp_launch_mode = "none";
                let detail = format!("Could not start Docker LSP from {image}: {error:#}");
                self.status_msg = Some(detail.clone());
                self.dialog = Some(Dialog::Message { text: detail });
            }
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

    async fn switch_lsp_document(&mut self, old_uri: &str) {
        if !self.lsp_available {
            return;
        }
        let new_uri = self.editor.uri();
        if old_uri == new_uri && !new_uri.is_empty() {
            let text = self.editor.text();
            let _ = self.lsp.change_file(&new_uri, &text).await;
        } else {
            if !old_uri.is_empty() {
                let _ = self.lsp.close_file(old_uri).await;
            }
            if !new_uri.is_empty() {
                let language_id = self.current_language.map(|l| l.id).unwrap_or("plaintext");
                let text = self.editor.text();
                let _ = self.lsp.open_file(&new_uri, &text, language_id).await;
            }
        }
        self.diagnostics.clear();
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
        let col = self.editor.lsp_character();
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
        let col = self.editor.lsp_character();
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
                self.editor.goto_line(target_row);
                self.editor.set_lsp_character(target_col as u32);
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
        let col = self.editor.lsp_character();
        match self.lsp.hover(&uri, row, col).await {
            Ok(text) if !text.trim().is_empty() => {
                self.dialog = Some(Dialog::Hover { text, scroll: 0 });
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

    async fn compile_project(&mut self) {
        if self.editor.modified {
            self.save_file();
            if self.editor.modified {
                return;
            }
        }
        let root = self.root_path.canonicalize().unwrap_or_else(|_| self.root_path.clone());
        let Some(source) = self.editor.file_path.clone().or_else(|| first_buildable_file(&self.tree_entries)) else {
            self.dialog = Some(Dialog::Message {
                text: "No buildable source file found in this project.".into(),
            });
            return;
        };
        let source = source.canonicalize().unwrap_or(source);
        let source_language = detect_language(&source);
        let compiler_override = source_language
            .and_then(|lang| self.settings.compiler_overrides.get(lang.id))
            .map(String::as_str);
        let Some(commands) = build_commands(&source, &root, false, &self.settings.run_parameters, compiler_override) else {
            self.dialog = Some(Dialog::Message {
                text: format!("No compile recipe for {}.", source.display()),
            });
            return;
        };

        let title = "Compile";
        let mut lines = Vec::new();
        let mut failed = false;
        for args in commands {
            lines.push(format!("$ {}", args.join(" ")));
            let mut command = Command::new(&args[0]);
            command.args(&args[1..]).current_dir(&root).kill_on_drop(true);
            let output = tokio::time::timeout(Duration::from_secs(120), command.output()).await;
            match output {
                Ok(Ok(output)) => {
                    lines.extend(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned));
                    lines.extend(String::from_utf8_lossy(&output.stderr).lines().map(str::to_owned));
                    if !output.status.success() {
                        lines.push(format!("Process exited with status {}", output.status));
                        failed = true;
                        break;
                    }
                }
                Ok(Err(error)) => {
                    lines.push(format!("Could not start {}: {error}", args[0]));
                    failed = true;
                    break;
                }
                Err(_) => {
                    lines.push("Process timed out after 120 seconds.".into());
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            lines.push("Completed successfully.".into());
            self.status_msg = Some(format!("{title} completed successfully."));
        } else {
            self.status_msg = Some(format!("{title} finished with errors."));
        }
        let diagnostics = parse_build_diagnostics(&lines, &root);
        self.dialog = Some(Dialog::BuildOutput {
            title: format!("{title} · {} diagnostic(s)", diagnostics.len()),
            lines,
            diagnostics,
            selected: 0,
        });
    }

    async fn run_program(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        if self.editor.modified {
            self.save_file();
            if self.editor.modified { return Ok(()); }
        }
        let root = self.root_path.canonicalize().unwrap_or_else(|_| self.root_path.clone());
        let Some(source) = self.editor.file_path.clone().or_else(|| first_buildable_file(&self.tree_entries)) else {
            self.dialog = Some(Dialog::Message { text: "No buildable source file found in this project.".into() });
            return Ok(());
        };
        let source = source.canonicalize().unwrap_or(source);
        let source_language = detect_language(&source);
        let compiler_override = source_language
            .and_then(|lang| self.settings.compiler_overrides.get(lang.id))
            .map(String::as_str);
        let Some(commands) = build_commands(&source, &root, true, &self.settings.run_parameters, compiler_override) else {
            self.dialog = Some(Dialog::Message { text: format!("No run recipe for {}.", source.display()) });
            return Ok(());
        };
        let mut lines = Vec::new();
        let split = commands.len().saturating_sub(1);
        for args in commands.iter().take(split) {
            lines.push(format!("$ {}", args.join(" ")));
            let mut command = Command::new(&args[0]);
            command.args(&args[1..]).current_dir(&root).kill_on_drop(true);
            match tokio::time::timeout(Duration::from_secs(120), command.output()).await {
                Ok(Ok(output)) => {
                    lines.extend(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned));
                    lines.extend(String::from_utf8_lossy(&output.stderr).lines().map(str::to_owned));
                    if !output.status.success() {
                        lines.push(format!("Build failed with status {}", output.status));
                        let diagnostics = parse_build_diagnostics(&lines, &root);
                        self.dialog = Some(Dialog::BuildOutput { title: "Run · build failed".into(), lines, diagnostics, selected: 0 });
                        return Ok(());
                    }
                }
                Ok(Err(error)) => {
                    lines.push(format!("Could not start {}: {error}", args[0]));
                    let diagnostics = parse_build_diagnostics(&lines, &root);
                    self.dialog = Some(Dialog::BuildOutput { title: "Run · build failed".into(), lines, diagnostics, selected: 0 });
                    return Ok(());
                }
                Err(_) => {
                    lines.push("Build timed out after 120 seconds.".into());
                    let diagnostics = parse_build_diagnostics(&lines, &root);
                    self.dialog = Some(Dialog::BuildOutput { title: "Run · build failed".into(), lines, diagnostics, selected: 0 });
                    return Ok(());
                }
            }
        }

        let run_command = &commands[commands.len() - 1];
        lines.push(format!("$ {}", run_command.join(" ")));
        terminal.show_cursor()?;
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::event::DisableMouseCapture)?;
        if self.enhanced_keyboard {
            crossterm::execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
        }
        let mut process = Command::new(&run_command[0]);
        process.args(&run_command[1..]).current_dir(&root);
        let status = status_with_interrupt_guard(&mut process).await;
        if self.enhanced_keyboard {
            crossterm::execute!(
                terminal.backend_mut(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
            )?;
        }
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture)?;
        terminal.clear()?;
        terminal.hide_cursor()?;
        match status {
            Ok(status) => {
                lines.push(format!("Program exited with status {status}."));
                self.status_msg = Some(format!("Program exited with {status}."));
            }
            Err(error) => {
                lines.push(format!("Could not start {}: {error}", run_command[0]));
                self.status_msg = Some("Could not start program.".into());
            }
        }
        let diagnostics = parse_build_diagnostics(&lines, &root);
        self.dialog = Some(Dialog::BuildOutput { title: "Run complete · Enter jumps to errors · Esc closes".into(), lines, diagnostics, selected: 0 });
        Ok(())
    }
}

fn build_commands(
    source: &Path,
    root: &Path,
    run: bool,
    params: &str,
    compiler_override: Option<&str>,
) -> Option<Vec<Vec<String>>> {
    let ext = source.extension()?.to_str()?.to_ascii_lowercase();
    let file = source.to_string_lossy().into_owned();
    let mut commands = Vec::new();
    let mut add = |parts: &[&str]| commands.push(parts.iter().map(|s| s.to_string()).collect());
    match ext.as_str() {
        "rs" if root.join("Cargo.toml").exists() => {
            if run { add(&["cargo", "run"]); } else { add(&["cargo", "check"]); }
        }
        "rs" => {
            let bin = std::env::temp_dir().join(format!("blueide-{}-program", std::process::id()));
            commands.push(vec!["rustc".into(), file, "-o".into(), bin.display().to_string()]);
            if run { commands.push(vec![bin.display().to_string()]); }
        }
        "kt" | "kts" if root.join("gradlew").exists() || root.join("build.gradle").exists() || root.join("build.gradle.kts").exists() => {
            let wrapper = if root.join("gradlew").exists() { "./gradlew" } else { "gradle" };
            if run { commands.push(vec![wrapper.into(), "run".into()]); }
            else { commands.push(vec![wrapper.into(), "build".into()]); }
        }
        "kt" | "kts" => {
            let jar = std::env::temp_dir().join(format!("blueide-{}-program.jar", std::process::id()));
            commands.push(vec!["kotlinc".into(), file, "-include-runtime".into(), "-d".into(), jar.display().to_string()]);
            if run { commands.push(vec!["java".into(), "-jar".into(), jar.display().to_string()]); }
        }
        "py" | "pyw" => {
            if run { commands.push(vec!["python3".into(), file]); }
            else {
                let cache = std::env::temp_dir().join(format!("blueide-{}-pycache", std::process::id()));
                commands.push(vec!["python3".into(), "-X".into(), format!("pycache_prefix={}", cache.display()), "-m".into(), "py_compile".into(), file]);
            }
        }
        "js" | "mjs" | "cjs" | "jsx" => {
            if run { commands.push(vec!["node".into(), file]); }
            else { commands.push(vec!["node".into(), "--check".into(), file]); }
        }
        "ts" | "tsx" => {
            if run { commands.push(vec!["tsx".into(), file]); }
            else { commands.push(vec!["tsc".into(), "--noEmit".into()]); }
        }
        "go" => {
            if run { commands.push(vec!["go".into(), "run".into(), file]); }
            else {
                let bin = std::env::temp_dir().join(format!("blueide-{}-go-program", std::process::id()));
                commands.push(vec!["go".into(), "build".into(), "-o".into(), bin.display().to_string(), file]);
            }
        }
        "c" | "cc" | "cpp" | "cxx" => {
            let bin = std::env::temp_dir().join(format!("blueide-{}-program", std::process::id()));
            let compiler = if ext == "c" { "cc" } else { "c++" };
            commands.push(vec![compiler.into(), file, "-o".into(), bin.display().to_string()]);
            if run { commands.push(vec![bin.display().to_string()]); }
        }
        "java" => {
            let stem = source.file_stem()?.to_string_lossy().into_owned();
            let classes = std::env::temp_dir().join(format!("blueide-{}-java", std::process::id()));
            let _ = std::fs::create_dir_all(&classes);
            commands.push(vec!["javac".into(), "-d".into(), classes.display().to_string(), file.clone()]);
            if run { commands.push(vec!["java".into(), "-cp".into(), classes.display().to_string(), stem]); }
        }
        "pas" | "pp" | "p" | "lpr" | "dpr" => {
            let out_dir = std::env::temp_dir().join(format!("blueide-{}-pascal", std::process::id()));
            let _ = std::fs::create_dir_all(&out_dir);
            commands.push(vec!["fpc".into(), format!("-FE{}", out_dir.display()), file.clone()]);
            if run {
                let exe_name = source.file_stem()?.to_string_lossy().into_owned();
                let exe = out_dir.join(executable_name(&exe_name));
                commands.push(vec![exe.display().to_string()]);
            }
        }
        _ => return None,
    }
    if run && !params.trim().is_empty() {
        if let Some(last) = commands.last_mut() {
            let arguments = parse_arguments(params);
            if last.first().is_some_and(|cmd| cmd == "cargo") && last.get(1).is_some_and(|arg| arg == "run") {
                last.push("--".into());
                last.extend(arguments);
            } else if last.first().is_some_and(|cmd| cmd.ends_with("gradle")) && last.get(1).is_some_and(|arg| arg == "run") {
                last.push(format!("--args={}", arguments.join(" ")));
            } else {
                last.extend(arguments);
            }
        }
    }
    if let (Some(compiler), Some(first)) = (
        compiler_override.filter(|s| !s.trim().is_empty()),
        commands.first_mut(),
    ) {
        let mut replacement = parse_arguments(compiler);
        if !replacement.is_empty() {
            first.splice(0..1, replacement.drain(..));
        }
    }
    Some(commands)
}

fn debugger_target(source: &Path, root: &Path) -> Option<(Vec<Vec<String>>, PathBuf)> {
    let ext = source.extension()?.to_str()?.to_ascii_lowercase();
    let file = source.to_string_lossy().into_owned();
    let id = std::process::id();
    match ext.as_str() {
        "rs" if root.join("Cargo.toml").exists() => {
            let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
            let mut in_package = false;
            let name = manifest.lines().find_map(|line| {
                let line = line.trim();
                if line.starts_with('[') {
                    in_package = line == "[package]";
                } else if in_package && line.starts_with("name") {
                    return Some(line.split_once('=')?.1.trim().trim_matches('"').to_string());
                }
                None
            })?;
            let binary = root.join("target").join("debug").join(executable_name(&name.replace('-', "_")));
            Some((vec![vec!["cargo".into(), "build".into()]], binary))
        }
        "rs" => {
            let binary = std::env::temp_dir().join(format!("blueide-{id}-debug"));
            Some((vec![vec!["rustc".into(), "-g".into(), file, "-o".into(), binary.display().to_string()]], binary))
        }
        "c" | "cc" | "cpp" | "cxx" => {
            let binary = std::env::temp_dir().join(format!("blueide-{id}-debug"));
            let compiler = if ext == "c" { "cc" } else { "c++" };
            Some((vec![vec![compiler.into(), "-g".into(), file, "-o".into(), binary.display().to_string()]], binary))
        }
        "pas" | "pp" | "p" | "lpr" | "dpr" => {
            let out_dir = std::env::temp_dir().join(format!("blueide-{id}-pascal-debug"));
            let _ = std::fs::create_dir_all(&out_dir);
            let stem = source.file_stem()?.to_string_lossy().into_owned();
            let binary = out_dir.join(executable_name(&stem));
            Some((vec![vec!["fpc".into(), "-g".into(), format!("-FE{}", out_dir.display()), file]], binary))
        }
        _ => None,
    }
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) { format!("{stem}.exe") } else { stem.to_string() }
}

async fn status_with_interrupt_guard(command: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    let signal_guard = tokio::spawn(async {
        loop {
            if tokio::signal::ctrl_c().await.is_err() { break; }
        }
    });
    let status = command.status().await;
    signal_guard.abort();
    status
}

fn first_buildable_file(entries: &[TreeEntry]) -> Option<PathBuf> {
    entries.iter().find_map(|entry| {
        if !entry.path.is_file() { return None; }
        let ext = entry.path.extension()?.to_str()?.to_ascii_lowercase();
        matches!(ext.as_str(), "rs" | "kt" | "kts" | "py" | "pyw" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "c" | "cc" | "cpp" | "cxx" | "java" | "pas" | "pp" | "p" | "lpr" | "dpr")
            .then(|| entry.path.clone())
    })
}

fn parse_arguments(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(open) = quote {
            if ch == open { quote = None; } else { current.push(ch); }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if escaped { current.push('\\'); }
    if !current.is_empty() { args.push(current); }
    args
}

fn parse_build_diagnostics(lines: &[String], root: &Path) -> Vec<BuildDiagnostic> {
    let mut result = Vec::new();
    for (output_line, line) in lines.iter().enumerate() {
        let trimmed = line.trim().trim_start_matches("-->").trim();
        let mut parsed = None;
        let parts: Vec<&str> = trimmed.split(':').collect();
        for index in 1..parts.len().saturating_sub(1) {
            if let (Ok(row), Ok(column)) = (parts[index].parse::<usize>(), parts[index + 1].parse::<usize>()) {
                let path = PathBuf::from(parts[..index].join(":"));
                if !path.as_os_str().is_empty() {
                    parsed = Some((path, row, column));
                    break;
                }
            }
        }
        if parsed.is_none() {
            if let Some((before, after)) = trimmed.split_once(", line ") {
                if let Ok(row) = after.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("").parse::<usize>() {
                    let path = before.trim_start_matches("File ").trim_matches('"');
                    parsed = Some((PathBuf::from(path), row, 1));
                }
            }
        }
        if parsed.is_none() {
            if let Some((path, rest)) = trimmed.split_once('(') {
                if let Some((position, _)) = rest.split_once(')') {
                    let mut coordinates = position.split(',');
                    if let (Some(Ok(row)), Some(Ok(column))) = (
                        coordinates.next().map(str::parse::<usize>),
                        coordinates.next().map(str::parse::<usize>),
                    ) {
                        parsed = Some((PathBuf::from(path), row, column));
                    }
                }
            }
        }
        if let Some((path, row, column)) = parsed {
            let path = if path.is_absolute() { path } else { root.join(path) };
            if path.is_file() {
                result.push(BuildDiagnostic { output_line, path, row: row.saturating_sub(1), column: column.saturating_sub(1) });
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// LSP server resolution helper
// ---------------------------------------------------------------------------

/// Build the server argv and a human-readable launch-mode label for a given
/// optional language.  Returns `(None, "none")` when no server is available.
fn menu_position(index: usize) -> usize {
    let labels = ["File", "Edit", "Search", "Run", "Options", "Window", "Help"];
    1 + labels.iter().take(index).map(|label| label.chars().count() + 2).sum::<usize>()
}

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

fn docker_image_from_argv(argv: &[String]) -> Option<String> {
    let network_index = argv.iter().position(|arg| arg == "--network")?;
    argv.get(network_index + 2).cloned()
}

// ---------------------------------------------------------------------------
// Tree builder
// ---------------------------------------------------------------------------

/// Build a flat list of file-tree entries for `root`, depth ≤ 8.
pub fn build_tree(root: &Path) -> Vec<TreeEntry> {
    let mut entries = Vec::new();
    collect_tree(root, root, 0, &mut entries);
    entries
}

fn collect_tree(root: &Path, dir: &Path, depth: usize, out: &mut Vec<TreeEntry>) {
    if depth > 8 {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<std::fs::DirEntry> = read_dir.flatten().collect();
    children.sort_by_key(|e| {
        let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
        (is_file, e.file_name())
    });

    for entry in children {
        let Ok(file_type) = entry.file_type() else { continue; };
        if file_type.is_symlink() { continue; }
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
        let prefix = if file_type.is_dir() { "▾ " } else { "  " };
        let display = format!("{indent}{prefix}{name_str}");
        out.push(TreeEntry { path: path.clone(), display });

        if file_type.is_dir() {
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
        'g' => Some(4), // Git
        'o' => Some(5), // Options
        'w' => Some(6), // Window
        'h' => Some(7), // Help
        _ => None,
    }
}

fn changed_lines_from_diff(diff: &str) -> HashSet<usize> {
    let mut changed = HashSet::new();
    for line in diff.lines().filter(|line| line.starts_with("@@ ")) {
        let Some((_, after_old)) = line.split_once(" +") else { continue; };
        let hunk = after_old.split_whitespace().next().unwrap_or("");
        let mut parts = hunk.split(',');
        let Ok(start) = parts.next().unwrap_or("").parse::<usize>() else { continue; };
        let count = parts.next().and_then(|part| part.parse::<usize>().ok()).unwrap_or(1);
        if count == 0 {
            changed.insert(start.saturating_sub(1));
        } else {
            let first = start.saturating_sub(1);
            changed.extend(first..first.saturating_add(count));
        }
    }
    changed
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
