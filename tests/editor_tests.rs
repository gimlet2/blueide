//! Integration tests for the editor buffer.

use blueide::editor::{highlight_line, Editor, TokenKind};

// ---------------------------------------------------------------------------
// Buffer construction
// ---------------------------------------------------------------------------

#[test]
fn default_editor_is_empty_single_line() {
    let ed = Editor::default();
    assert_eq!(ed.line_count(), 1);
    assert_eq!(ed.lines()[0], "");
}

#[test]
fn with_text_splits_lines() {
    let ed = Editor::with_text("first\nsecond\nthird");
    assert_eq!(ed.line_count(), 3);
    assert_eq!(ed.lines()[2], "third");
}

#[test]
fn text_roundtrip() {
    let src = "fun main() {\n    println(\"hello\")\n}";
    let ed = Editor::with_text(src);
    assert_eq!(ed.text(), src);
}

// ---------------------------------------------------------------------------
// Editing operations
// ---------------------------------------------------------------------------

#[test]
fn insert_char_into_empty_buffer() {
    let mut ed = Editor::default();
    ed.insert_char('K');
    assert_eq!(ed.lines()[0], "K");
    assert_eq!(ed.cursor_col, 1);
}

#[test]
fn insert_multiple_chars() {
    let mut ed = Editor::default();
    for ch in "hello".chars() {
        ed.insert_char(ch);
    }
    assert_eq!(ed.lines()[0], "hello");
    assert_eq!(ed.cursor_col, 5);
}

#[test]
fn insert_newline_splits_at_cursor() {
    let mut ed = Editor::with_text("abcdef");
    ed.cursor_col = 3;
    ed.insert_newline();
    assert_eq!(ed.lines()[0], "abc");
    assert_eq!(ed.lines()[1], "def");
    assert_eq!(ed.cursor_row, 1);
    assert_eq!(ed.cursor_col, 0);
}

#[test]
fn insert_newline_at_end_creates_blank_line() {
    let mut ed = Editor::with_text("abc");
    ed.cursor_col = 3;
    ed.insert_newline();
    assert_eq!(ed.line_count(), 2);
    assert_eq!(ed.lines()[1], "");
}

#[test]
fn delete_backward_mid_line() {
    let mut ed = Editor::with_text("hello");
    ed.cursor_col = 3;
    ed.delete_backward();
    assert_eq!(ed.lines()[0], "helo");
    assert_eq!(ed.cursor_col, 2);
}

#[test]
fn delete_backward_joins_lines() {
    let mut ed = Editor::with_text("foo\nbar");
    ed.cursor_row = 1;
    ed.cursor_col = 0;
    ed.delete_backward();
    assert_eq!(ed.line_count(), 1);
    assert_eq!(ed.lines()[0], "foobar");
    assert_eq!(ed.cursor_col, 3);
}

#[test]
fn delete_forward_mid_line() {
    let mut ed = Editor::with_text("hello");
    ed.cursor_col = 2;
    ed.delete_forward();
    assert_eq!(ed.lines()[0], "helo");
}

#[test]
fn delete_forward_joins_lines() {
    let mut ed = Editor::with_text("foo\nbar");
    ed.cursor_row = 0;
    ed.cursor_col = 3; // end of "foo"
    ed.delete_forward();
    assert_eq!(ed.line_count(), 1);
    assert_eq!(ed.lines()[0], "foobar");
}

// ---------------------------------------------------------------------------
// Cursor movement
// ---------------------------------------------------------------------------

#[test]
fn move_up_clamps_at_first_line() {
    let mut ed = Editor::with_text("line1\nline2");
    ed.cursor_row = 0;
    ed.move_up();
    assert_eq!(ed.cursor_row, 0);
}

#[test]
fn move_down_clamps_at_last_line() {
    let mut ed = Editor::with_text("line1\nline2");
    ed.cursor_row = 1;
    ed.move_down();
    assert_eq!(ed.cursor_row, 1);
}

#[test]
fn move_left_wraps_to_previous_line_end() {
    let mut ed = Editor::with_text("hello\nworld");
    ed.cursor_row = 1;
    ed.cursor_col = 0;
    ed.move_left();
    assert_eq!(ed.cursor_row, 0);
    assert_eq!(ed.cursor_col, 5); // end of "hello"
}

#[test]
fn move_right_wraps_to_next_line_start() {
    let mut ed = Editor::with_text("hi\nbye");
    ed.cursor_row = 0;
    ed.cursor_col = 2; // end of "hi"
    ed.move_right();
    assert_eq!(ed.cursor_row, 1);
    assert_eq!(ed.cursor_col, 0);
}

#[test]
fn move_home_goes_to_first_non_whitespace() {
    let mut ed = Editor::with_text("    val x = 1");
    ed.cursor_col = 10;
    ed.move_home();
    assert_eq!(ed.cursor_col, 4); // first non-ws
    ed.move_home();
    assert_eq!(ed.cursor_col, 0); // column 0 on second press
}

#[test]
fn move_end_goes_to_line_end() {
    let mut ed = Editor::with_text("hello world");
    ed.cursor_col = 0;
    ed.move_end();
    assert_eq!(ed.cursor_col, 11);
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

#[test]
fn undo_reverts_insert_char() {
    let mut ed = Editor::with_text("abc");
    ed.cursor_col = 3;
    ed.insert_char('!');
    assert_eq!(ed.lines()[0], "abc!");
    ed.undo();
    assert_eq!(ed.lines()[0], "abc");
}

#[test]
fn undo_when_empty_does_not_panic() {
    let mut ed = Editor::default();
    ed.undo(); // no-op
}

// ---------------------------------------------------------------------------
// Modified flag
// ---------------------------------------------------------------------------

#[test]
fn modified_flag_set_on_insert() {
    let mut ed = Editor::with_text("abc");
    assert!(!ed.modified);
    ed.insert_char('x');
    assert!(ed.modified);
}

// ---------------------------------------------------------------------------
// Syntax highlighting
// ---------------------------------------------------------------------------

#[test]
fn highlight_kotlin_fun_keyword() {
    let (kinds, _) = highlight_line("fun greet() {}", false);
    // 'f','u','n' are all keyword.
    assert_eq!(kinds[0], TokenKind::Keyword);
    assert_eq!(kinds[1], TokenKind::Keyword);
    assert_eq!(kinds[2], TokenKind::Keyword);
}

#[test]
fn highlight_val_keyword() {
    let (kinds, _) = highlight_line("val x = 5", false);
    assert_eq!(kinds[0], TokenKind::Keyword); // v
    assert_eq!(kinds[1], TokenKind::Keyword); // a
    assert_eq!(kinds[2], TokenKind::Keyword); // l
}

#[test]
fn highlight_string_with_escape() {
    let (kinds, _) = highlight_line(r#""hello\nworld""#, false);
    assert!(kinds.iter().all(|k| *k == TokenKind::StringLiteral));
}

#[test]
fn highlight_line_comment_marks_all_as_comment() {
    let (kinds, _) = highlight_line("// This is a comment", false);
    assert!(kinds.iter().all(|k| *k == TokenKind::Comment));
}

#[test]
fn highlight_block_comment_open_sets_in_block() {
    let (_kinds, still_in) = highlight_line("/* block", false);
    assert!(still_in);
}

#[test]
fn highlight_block_comment_close_resets_in_block() {
    let (_kinds, still_in) = highlight_line("  end of block */", true);
    assert!(!still_in);
}

#[test]
fn highlight_annotation() {
    let (kinds, _) = highlight_line("@JvmStatic fun test()", false);
    // @JvmStatic → Annotation
    assert_eq!(kinds[0], TokenKind::Annotation);
}

#[test]
fn highlight_number() {
    let (kinds, _) = highlight_line("val n = 42", false);
    // '4' and '2' are numbers
    let num_indices: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| **k == TokenKind::Number)
        .map(|(i, _)| i)
        .collect();
    assert!(!num_indices.is_empty(), "expected number tokens");
}

#[test]
fn highlight_mixed_line() {
    // fun printHello() { println("Hello") // greet }
    let (kinds, _) = highlight_line(
        r#"fun printHello() { println("Hello") // greet }"#,
        false,
    );
    // 'fun' → Keyword
    assert_eq!(kinds[0], TokenKind::Keyword);
    // After '//' everything should be Comment
    let comment_start = kinds.iter().position(|k| *k == TokenKind::Comment);
    assert!(comment_start.is_some());
    let comment_start = comment_start.unwrap();
    for k in &kinds[comment_start..] {
        assert_eq!(*k, TokenKind::Comment);
    }
}

// ---------------------------------------------------------------------------
// Scrolling
// ---------------------------------------------------------------------------

#[test]
fn scroll_keeps_cursor_visible_vertically() {
    let content: String = (0..50).map(|i| format!("line {i}\n")).collect();
    let mut ed = Editor::with_text(&content);
    ed.cursor_row = 40;
    ed.scroll_to_cursor(10, 80);
    assert!(ed.scroll_row <= 40);
    assert!(ed.scroll_row + 10 > 40);
}

#[test]
fn scroll_adjusts_when_cursor_above_viewport() {
    let content: String = (0..30).map(|i| format!("line {i}\n")).collect();
    let mut ed = Editor::with_text(&content);
    ed.scroll_row = 20;
    ed.cursor_row = 5;
    ed.scroll_to_cursor(10, 80);
    assert_eq!(ed.scroll_row, 5);
}

// ---------------------------------------------------------------------------
// Redo
// ---------------------------------------------------------------------------

#[test]
fn redo_reapplies_undone_change() {
    let mut ed = Editor::with_text("abc");
    ed.cursor_col = 3;
    ed.insert_char('!');
    assert_eq!(ed.lines()[0], "abc!");
    ed.undo();
    assert_eq!(ed.lines()[0], "abc");
    ed.redo();
    assert_eq!(ed.lines()[0], "abc!");
}

#[test]
fn new_edit_clears_redo_stack() {
    let mut ed = Editor::with_text("abc");
    ed.cursor_col = 3;
    ed.insert_char('!');
    ed.undo();
    // Make a new edit — redo should now be a no-op.
    ed.insert_char('?');
    ed.redo(); // should not restore "abc!"
    assert_eq!(ed.lines()[0], "abc?");
}

// ---------------------------------------------------------------------------
// Copy / Cut line
// ---------------------------------------------------------------------------

#[test]
fn copy_line_returns_current_line_text() {
    let mut ed = Editor::with_text("hello\nworld");
    ed.cursor_row = 0;
    assert_eq!(ed.copy_line(), "hello");
    ed.cursor_row = 1;
    assert_eq!(ed.copy_line(), "world");
}

#[test]
fn cut_line_removes_line_and_returns_text() {
    let mut ed = Editor::with_text("alpha\nbeta\ngamma");
    ed.cursor_row = 1;
    let text = ed.cut_line();
    assert_eq!(text, "beta");
    assert_eq!(ed.line_count(), 2);
    assert_eq!(ed.lines()[0], "alpha");
    assert_eq!(ed.lines()[1], "gamma");
}

#[test]
fn cut_line_single_line_clears_content() {
    let mut ed = Editor::with_text("only");
    let text = ed.cut_line();
    assert_eq!(text, "only");
    assert_eq!(ed.line_count(), 1);
    assert_eq!(ed.lines()[0], "");
}

// ---------------------------------------------------------------------------
// Go to Line
// ---------------------------------------------------------------------------

#[test]
fn goto_line_moves_cursor_to_row() {
    let mut ed = Editor::with_text("a\nb\nc\nd\ne");
    ed.goto_line(3);
    assert_eq!(ed.cursor_row, 3);
}

#[test]
fn goto_line_clamps_to_last_line() {
    let mut ed = Editor::with_text("a\nb\nc");
    ed.goto_line(100);
    assert_eq!(ed.cursor_row, 2);
}

// ---------------------------------------------------------------------------
// Replace next
// ---------------------------------------------------------------------------

#[test]
fn replace_next_substitutes_first_occurrence() {
    let mut ed = Editor::with_text("foo bar foo");
    assert!(ed.replace_next("foo", "baz", 0));
    assert_eq!(ed.lines()[0], "baz bar foo");
}

#[test]
fn replace_next_returns_false_when_not_found() {
    let mut ed = Editor::with_text("hello world");
    assert!(!ed.replace_next("xyz", "abc", 0));
    assert_eq!(ed.lines()[0], "hello world");
}

#[test]
fn replace_next_wraps_around_from_row() {
    let mut ed = Editor::with_text("first\ntarget\nlast");
    // Start searching from row 2 — should wrap and find "target" on row 1.
    assert!(ed.replace_next("target", "found", 2));
    assert_eq!(ed.lines()[1], "found");
}
