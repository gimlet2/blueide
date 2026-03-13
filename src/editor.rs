//! Text editor buffer with cursor, viewport, and simple text operations.

use std::path::PathBuf;

/// Maximum columns to keep in the undo stack.
const MAX_UNDO: usize = 100;

/// A line-based text buffer with a cursor and scroll viewport.
#[derive(Debug, Clone)]
pub struct Editor {
    /// Text content, one entry per line.
    lines: Vec<String>,
    /// Zero-based cursor row.
    pub cursor_row: usize,
    /// Zero-based cursor column (byte offset within the line).
    pub cursor_col: usize,
    /// First visible row (vertical scroll offset).
    pub scroll_row: usize,
    /// First visible column (horizontal scroll offset).
    pub scroll_col: usize,
    /// Path of the file currently open in this buffer.
    pub file_path: Option<PathBuf>,
    /// Whether the buffer has unsaved changes.
    pub modified: bool,
    /// Simple undo history: snapshots of (lines, cursor_row, cursor_col).
    undo_stack: Vec<(Vec<String>, usize, usize)>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            file_path: None,
            modified: false,
            undo_stack: Vec::new(),
        }
    }
}

impl Editor {
    // ------------------------------------------------------------------
    // Construction / I/O
    // ------------------------------------------------------------------

    /// Create an editor pre-loaded with `text`.
    pub fn with_text(text: impl AsRef<str>) -> Self {
        let mut ed = Self::default();
        ed.set_text(text.as_ref());
        ed
    }

    /// Load a file from disk into the buffer.
    pub fn load_file(path: PathBuf) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(&path)?;
        let mut ed = Self::with_text(&text);
        ed.file_path = Some(path);
        ed.modified = false;
        Ok(ed)
    }

    /// Save the buffer to its current [`file_path`].
    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self.file_path.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no file path set")
        })?;
        std::fs::write(path, self.text())?;
        self.modified = false;
        Ok(())
    }

    /// Save to an explicit path (and update [`file_path`]).
    pub fn save_as(&mut self, path: PathBuf) -> std::io::Result<()> {
        std::fs::write(&path, self.text())?;
        self.file_path = Some(path);
        self.modified = false;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Content access
    // ------------------------------------------------------------------

    /// Return all lines as a slice.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Return the full buffer as a single string.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Replace the entire buffer content with `text`.
    pub fn set_text(&mut self, text: &str) {
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.modified = false;
    }

    /// Return the display name: file name or `[No Name]`.
    pub fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[No Name]".into())
    }

    /// The LSP URI for the current file (`file:///...`), or empty string.
    pub fn uri(&self) -> String {
        match &self.file_path {
            Some(p) => crate::lsp::url_from_path(p),
            None => String::new(),
        }
    }

    // ------------------------------------------------------------------
    // Cursor movement
    // ------------------------------------------------------------------

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col();
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.clamp_col();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_col < self.current_line_len() {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_home(&mut self) {
        // Move to first non-whitespace, then to column 0 on second press.
        let first_non_ws = self.lines[self.cursor_row]
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        if self.cursor_col != first_non_ws {
            self.cursor_col = first_non_ws;
        } else {
            self.cursor_col = 0;
        }
    }

    pub fn move_end(&mut self) {
        self.cursor_col = self.current_line_len();
    }

    pub fn move_page_up(&mut self, page_height: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(page_height);
        self.clamp_col();
    }

    pub fn move_page_down(&mut self, page_height: usize) {
        self.cursor_row =
            (self.cursor_row + page_height).min(self.lines.len().saturating_sub(1));
        self.clamp_col();
    }

    pub fn move_word_right(&mut self) {
        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut col = self.cursor_col;
        // Skip current word characters.
        while col < chars.len() && chars[col].is_alphanumeric() {
            col += 1;
        }
        // Skip whitespace.
        while col < chars.len() && !chars[col].is_alphanumeric() {
            col += 1;
        }
        self.cursor_col = col;
    }

    pub fn move_word_left(&mut self) {
        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut col = self.cursor_col;
        if col == 0 {
            return;
        }
        col -= 1;
        // Skip whitespace to the left.
        while col > 0 && !chars[col].is_alphanumeric() {
            col -= 1;
        }
        // Skip word to the left.
        while col > 0 && chars[col - 1].is_alphanumeric() {
            col -= 1;
        }
        self.cursor_col = col;
    }

    // ------------------------------------------------------------------
    // Text editing operations
    // ------------------------------------------------------------------

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.push_undo();
        let col = self.cursor_col;
        let line = &mut self.lines[self.cursor_row];
        // Ensure we insert at a valid char boundary.
        let byte_pos = char_to_byte(line, col);
        line.insert(byte_pos, ch);
        self.cursor_col += 1;
        self.modified = true;
    }

    /// Insert a newline, splitting the current line at the cursor.
    pub fn insert_newline(&mut self) {
        self.push_undo();
        let col = self.cursor_col;
        let byte_pos = char_to_byte(&self.lines[self.cursor_row], col);
        let rest = self.lines[self.cursor_row].split_off(byte_pos);
        // Preserve indentation of current line.
        let indent: String = self.lines[self.cursor_row]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let new_line = indent + &rest;
        let indent_len = new_line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        self.cursor_row += 1;
        self.cursor_col = indent_len;
        self.lines.insert(self.cursor_row, new_line);
        self.modified = true;
    }

    /// Delete the character before the cursor (Backspace).
    pub fn delete_backward(&mut self) {
        self.push_undo();
        if self.cursor_col > 0 {
            let byte_pos = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            // Find previous char boundary.
            let prev = prev_char_boundary(&self.lines[self.cursor_row], byte_pos);
            self.lines[self.cursor_row].remove(prev);
            self.cursor_col -= 1;
            self.modified = true;
        } else if self.cursor_row > 0 {
            // Join with previous line.
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_len = self.lines[self.cursor_row].chars().count();
            self.lines[self.cursor_row].push_str(&current);
            self.cursor_col = prev_len;
            self.modified = true;
        }
    }

    /// Delete the character under the cursor (Delete).
    pub fn delete_forward(&mut self) {
        self.push_undo();
        if self.cursor_col < self.current_line_len() {
            let byte_pos = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_pos);
            self.modified = true;
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.modified = true;
        }
    }

    /// Insert a string (e.g. completion text) at the cursor.
    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(ch);
            }
        }
    }

    // ------------------------------------------------------------------
    // Undo
    // ------------------------------------------------------------------

    pub fn undo(&mut self) {
        if let Some((lines, row, col)) = self.undo_stack.pop() {
            self.lines = lines;
            self.cursor_row = row;
            self.cursor_col = col;
            self.modified = true;
        }
    }

    fn push_undo(&mut self) {
        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.undo_stack
            .push((self.lines.clone(), self.cursor_row, self.cursor_col));
    }

    // ------------------------------------------------------------------
    // Viewport / scrolling
    // ------------------------------------------------------------------

    /// Adjust the scroll offsets so the cursor is within the visible area.
    pub fn scroll_to_cursor(&mut self, visible_rows: usize, visible_cols: usize) {
        // Vertical.
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + visible_rows {
            self.scroll_row = self.cursor_row + 1 - visible_rows;
        }
        // Horizontal.
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + visible_cols {
            self.scroll_col = self.cursor_col + 1 - visible_cols;
        }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn current_line_len(&self) -> usize {
        self.lines[self.cursor_row].chars().count()
    }

    fn clamp_col(&mut self) {
        let max = self.current_line_len();
        if self.cursor_col > max {
            self.cursor_col = max;
        }
    }
}

/// Convert a char-index `col` to a byte offset in `s`.
fn char_to_byte(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

/// Return the byte offset of the previous UTF-8 character boundary.
fn prev_char_boundary(s: &str, byte_pos: usize) -> usize {
    let mut pos = byte_pos.saturating_sub(1);
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

// ---------------------------------------------------------------------------
// Kotlin syntax highlighting helper
// ---------------------------------------------------------------------------

/// Classify each character position in a line for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Normal,
    Keyword,
    Comment,
    StringLiteral,
    Number,
    Annotation,
}

/// Returns a vector of `TokenKind` for each character in `line`.
pub fn highlight_line(line: &str, in_block_comment: bool) -> (Vec<TokenKind>, bool) {
    const KEYWORDS: &[&str] = &[
        "abstract", "actual", "annotation", "as", "break", "by", "catch", "class",
        "companion", "const", "constructor", "continue", "crossinline", "data",
        "delegate", "do", "dynamic", "else", "enum", "expect", "external",
        "false", "field", "file", "final", "finally", "for", "fun", "get",
        "if", "import", "in", "infix", "init", "inline", "inner", "interface",
        "internal", "is", "it", "lateinit", "noinline", "null", "object",
        "open", "operator", "out", "override", "package", "param", "private",
        "protected", "public", "receiver", "reified", "return", "sealed",
        "set", "setparam", "super", "suspend", "tailrec", "this", "throw",
        "true", "try", "typealias", "typeof", "val", "value", "var", "vararg",
        "when", "where", "while",
    ];

    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut kinds = vec![TokenKind::Normal; len];
    let mut i = 0;
    let mut in_block = in_block_comment;
    let mut in_string = false;
    let mut in_char = false;

    while i < len {
        if in_block {
            kinds[i] = TokenKind::Comment;
            if i + 1 < len && chars[i] == '*' && chars[i + 1] == '/' {
                kinds[i + 1] = TokenKind::Comment;
                i += 2;
                in_block = false;
            } else {
                i += 1;
            }
            continue;
        }

        // Line comment.
        if !in_string && !in_char && i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            for j in i..len {
                kinds[j] = TokenKind::Comment;
            }
            break;
        }

        // Block comment start.
        if !in_string && !in_char && i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            kinds[i] = TokenKind::Comment;
            kinds[i + 1] = TokenKind::Comment;
            i += 2;
            in_block = true;
            continue;
        }

        // String literal.
        if chars[i] == '"' && !in_char {
            in_string = !in_string;
            kinds[i] = TokenKind::StringLiteral;
            i += 1;
            continue;
        }
        if in_string {
            kinds[i] = TokenKind::StringLiteral;
            if chars[i] == '\\' && i + 1 < len {
                kinds[i + 1] = TokenKind::StringLiteral;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // Char literal.
        if chars[i] == '\'' {
            in_char = !in_char;
            kinds[i] = TokenKind::StringLiteral;
            i += 1;
            continue;
        }
        if in_char {
            kinds[i] = TokenKind::StringLiteral;
            if chars[i] == '\\' && i + 1 < len {
                kinds[i + 1] = TokenKind::StringLiteral;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // Annotation.
        if chars[i] == '@' {
            let start = i;
            i += 1;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            for j in start..i {
                kinds[j] = TokenKind::Annotation;
            }
            continue;
        }

        // Number literal.
        if chars[i].is_ascii_digit()
            || (chars[i] == '-'
                && i + 1 < len
                && chars[i + 1].is_ascii_digit()
                && (i == 0 || !chars[i - 1].is_alphanumeric()))
        {
            let start = i;
            i += 1;
            while i < len
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            for j in start..i {
                kinds[j] = TokenKind::Number;
            }
            continue;
        }

        // Identifier / keyword.
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let kind = if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Keyword
            } else {
                TokenKind::Normal
            };
            for j in start..i {
                kinds[j] = kind;
            }
            continue;
        }

        i += 1;
    }

    (kinds, in_block)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_editor(text: &str) -> Editor {
        Editor::with_text(text)
    }

    #[test]
    fn default_editor_has_one_empty_line() {
        let ed = Editor::default();
        assert_eq!(ed.line_count(), 1);
        assert_eq!(ed.lines()[0], "");
        assert_eq!(ed.cursor_row, 0);
        assert_eq!(ed.cursor_col, 0);
    }

    #[test]
    fn insert_char_advances_cursor() {
        let mut ed = Editor::default();
        ed.insert_char('h');
        ed.insert_char('i');
        assert_eq!(ed.lines()[0], "hi");
        assert_eq!(ed.cursor_col, 2);
    }

    #[test]
    fn insert_newline_splits_line() {
        let mut ed = make_editor("hello");
        ed.cursor_col = 2;
        ed.insert_newline();
        assert_eq!(ed.line_count(), 2);
        assert_eq!(ed.lines()[0], "he");
        assert_eq!(ed.lines()[1], "llo");
        assert_eq!(ed.cursor_row, 1);
        assert_eq!(ed.cursor_col, 0);
    }

    #[test]
    fn delete_backward_removes_char() {
        let mut ed = make_editor("hello");
        ed.cursor_col = 5;
        ed.delete_backward();
        assert_eq!(ed.lines()[0], "hell");
        assert_eq!(ed.cursor_col, 4);
    }

    #[test]
    fn delete_backward_at_line_start_joins_lines() {
        let mut ed = make_editor("he\nllo");
        ed.cursor_row = 1;
        ed.cursor_col = 0;
        ed.delete_backward();
        assert_eq!(ed.line_count(), 1);
        assert_eq!(ed.lines()[0], "hello");
        assert_eq!(ed.cursor_row, 0);
        assert_eq!(ed.cursor_col, 2);
    }

    #[test]
    fn delete_forward_removes_char_under_cursor() {
        let mut ed = make_editor("hello");
        ed.cursor_col = 0;
        ed.delete_forward();
        assert_eq!(ed.lines()[0], "ello");
        assert_eq!(ed.cursor_col, 0);
    }

    #[test]
    fn move_up_down_clamps_col() {
        let mut ed = make_editor("hello\nhi");
        ed.cursor_row = 0;
        ed.cursor_col = 5;
        ed.move_down(); // line 1 has only 2 chars
        assert_eq!(ed.cursor_row, 1);
        assert_eq!(ed.cursor_col, 2);
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut ed = make_editor("hello");
        ed.cursor_col = 5;
        ed.insert_char('!');
        assert_eq!(ed.lines()[0], "hello!");
        ed.undo();
        assert_eq!(ed.lines()[0], "hello");
        assert_eq!(ed.cursor_col, 5);
    }

    #[test]
    fn text_roundtrip() {
        let src = "fun main() {\n    println(\"hi\")\n}";
        let ed = make_editor(src);
        assert_eq!(ed.text(), src);
    }

    #[test]
    fn highlight_kotlin_keywords() {
        let (kinds, _) = highlight_line("fun main() {", false);
        // "fun" → keyword
        assert_eq!(kinds[0], TokenKind::Keyword);
        assert_eq!(kinds[1], TokenKind::Keyword);
        assert_eq!(kinds[2], TokenKind::Keyword);
        // space after 'fun'
        assert_eq!(kinds[3], TokenKind::Normal);
    }

    #[test]
    fn highlight_line_comment() {
        let (kinds, _) = highlight_line("// comment text", false);
        assert!(kinds.iter().all(|k| *k == TokenKind::Comment));
    }

    #[test]
    fn highlight_string_literal() {
        let (kinds, _) = highlight_line("\"hello\"", false);
        assert!(kinds.iter().all(|k| *k == TokenKind::StringLiteral));
    }

    #[test]
    fn highlight_block_comment_spans_lines() {
        let (_kinds1, in_block) = highlight_line("/* start", false);
        assert!(in_block);
        let (_kinds2, in_block2) = highlight_line("still comment */", true);
        assert!(!in_block2);
    }

    #[test]
    fn highlight_annotation() {
        let (kinds, _) = highlight_line("@Override", false);
        assert_eq!(kinds[0], TokenKind::Annotation);
    }

    #[test]
    fn scroll_to_cursor_adjusts_scroll_row() {
        let mut ed = make_editor("a\nb\nc\nd\ne");
        ed.cursor_row = 4;
        ed.scroll_to_cursor(3, 80);
        assert!(ed.scroll_row <= ed.cursor_row);
        assert!(ed.scroll_row + 3 > ed.cursor_row);
    }
}
