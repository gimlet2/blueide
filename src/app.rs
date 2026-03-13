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
use crate::lsp::{CompletionItem, Diagnostic, LspClient, LspEvent};

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
    OpenFile { input: String },
    SaveAs { input: String },
    Find { input: String, from_row: usize },
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
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub editor: Editor,
    pub focus: FocusArea,
    pub menu_index: usize,
    pub show_tree: bool,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub dialog: Option<Dialog>,
    pub completion: CompletionState,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub lsp_available: bool,
    pub status_msg: Option<String>,
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

        let lsp = LspClient::new(root_path.clone(), None);
        let tree_entries = build_tree(&root_path);

        Ok(Self {
            editor,
            focus: FocusArea::Editor,
            menu_index: 0,
            show_tree: true,
            tree_entries,
            tree_selected: 0,
            dialog: None,
            completion: CompletionState::Hidden,
            diagnostics: HashMap::new(),
            lsp_available: false,
            status_msg: None,
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
            let _ = self.lsp.open_file(&uri, &text, "kotlin").await;
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
                    input: String::new(),
                    from_row: self.editor.cursor_row,
                });
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

            // ---- Undo ----
            KeyCode::Char('z') if ctrl => {
                self.editor.undo();
                self.notify_lsp_change().await;
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
        const MENU_COUNT: usize = 7;
        match key.code {
            KeyCode::Esc | KeyCode::F(10) => {
                self.focus = FocusArea::Editor;
            }
            KeyCode::Left => {
                self.menu_index = self.menu_index.saturating_sub(1);
            }
            KeyCode::Right => {
                self.menu_index = (self.menu_index + 1).min(MENU_COUNT - 1);
            }
            KeyCode::Enter => {
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            // Hotkeys
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.menu_index = 0;
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                self.menu_index = 1;
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.menu_index = 2;
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.menu_index = 3;
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                self.menu_index = 6;
                self.activate_menu_item().await?;
                self.focus = FocusArea::Editor;
            }
            _ => {}
        }
        Ok(false)
    }

    async fn activate_menu_item(&mut self) -> Result<()> {
        match self.menu_index {
            0 => {
                // File → Open
                let cur = self
                    .editor
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::OpenFile { input: cur });
            }
            1 => {} // Edit — no-op for now
            2 => {
                // Search → Find
                self.dialog = Some(Dialog::Find {
                    input: String::new(),
                    from_row: self.editor.cursor_row,
                });
            }
            3 => {
                // Run
                self.run_project().await;
            }
            6 => {
                self.dialog = Some(Dialog::Help);
            }
            _ => {}
        }
        Ok(())
    }

    /// Activate a menu item by its Alt+letter hotkey.
    ///
    /// Maps the hotkey character (case-insensitive) to a menu index, fires the
    /// corresponding action, and returns focus to the editor.  Returns `true`
    /// when a matching item was found, `false` otherwise (so the key falls
    /// through to normal handling).
    async fn activate_menu_by_hotkey(&mut self, ch: char) -> bool {
        let idx = match ch.to_ascii_lowercase() {
            'f' => 0, // File
            'e' => 1, // Edit
            's' => 2, // Search
            'r' => 3, // Run
            'o' => 4, // Options
            'w' => 5, // Window
            'h' => 6, // Help
            _ => return false,
        };
        self.menu_index = idx;
        let _ = self.activate_menu_item().await;
        self.focus = FocusArea::Editor;
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
            Some(Dialog::Help) => {
                // Any key closes help.
                // dialog already taken (None)
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

    fn open_file_path(&mut self, path: PathBuf) {
        match Editor::load_file(path) {
            Ok(ed) => {
                self.editor = ed;
                self.status_msg = Some("File opened.".into());
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

    // ------------------------------------------------------------------
    // LSP helpers
    // ------------------------------------------------------------------

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
        let _rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        out.push(TreeEntry { path: path.clone(), display });

        if path.is_dir() {
            collect_tree(root, &path, depth + 1, out);
        }
    }
}
