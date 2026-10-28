//! Ratatui-based UI renderer — Turbo Pascal blue theme.
//!
//! Layout:
//!   ┌──────────────────────────────┐
//!   │ Menu bar (gray)              │  1 row
//!   ├────────┬─────────────────────┤
//!   │ File   │                     │
//!   │ tree   │  Editor             │  fill
//!   │        │                     │
//!   ├────────┴─────────────────────┤
//!   │ Status bar (cyan)            │  1 row
//!   └──────────────────────────────┘

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, BuildDiagnostic, CompletionState, Dialog, FocusArea, HelpLine, SettingKind, SubMenuItem, SUBMENUS};
use crate::editor::{highlight_line_for_language, TokenKind};
use crate::lsp::{Diagnostic, DiagnosticSeverity};

// ---------------------------------------------------------------------------
// Colour palette — authentic Turbo Pascal blue
// ---------------------------------------------------------------------------

/// Classic TP dark-blue editor background (#0000AA).
pub const C_BLUE: Color = Color::Rgb(0, 0, 170);
/// Slightly lighter blue for panels.
pub const C_BLUE_LIGHT: Color = Color::Rgb(0, 0, 200);
/// Bright white text.
pub const C_WHITE: Color = Color::Rgb(255, 255, 255);
/// Gray for menu bar / borders.
pub const C_GRAY: Color = Color::Rgb(170, 170, 170);
/// Dark gray — inactive UI elements.
pub const C_DARK_GRAY: Color = Color::Rgb(85, 85, 85);
/// Cyan highlight (selected menu item, status bar).
pub const C_CYAN: Color = Color::Rgb(0, 170, 170);
/// Bright yellow — cursor line, matched items.
pub const C_YELLOW: Color = Color::Rgb(255, 255, 85);
/// Bright red for errors.
pub const C_RED: Color = Color::Rgb(255, 85, 85);
/// Orange/yellow for warnings.
pub const C_ORANGE: Color = Color::Rgb(255, 170, 0);
/// Green for info / annotations.
pub const C_GREEN: Color = Color::Rgb(85, 255, 85);
/// Black text on bright backgrounds.
pub const C_BLACK: Color = Color::Rgb(0, 0, 0);
/// Bright yellow reserved words for a stronger classic DOS syntax palette.
pub const C_KW: Color = Color::Rgb(255, 255, 85);
/// Light cyan strings stand out against the DOS blue editor.
pub const C_STR: Color = Color::Rgb(85, 255, 255);
/// Muted teal comments keep executable code prominent.
pub const C_COMMENT: Color = Color::Rgb(85, 170, 170);
/// Yellow numeric constants.
pub const C_NUM: Color = Color::Rgb(255, 255, 85);
/// Magenta annotations and decorators.
pub const C_ANNOT: Color = Color::Rgb(255, 85, 255);

// ---------------------------------------------------------------------------
// Main render entry-point
// ---------------------------------------------------------------------------

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Vertical split: menu | main | status
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // menu bar
            Constraint::Length(1), // open buffer bar
            Constraint::Min(0),    // main area
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let menu_area = rows[0];
    let buffer_area = rows[1];
    let main_area = rows[2];
    let status_area = rows[3];

    // Horizontal split: file tree | editor (when tree is visible)
    let (tree_area, editor_area) = if app.show_tree {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(0)])
            .split(main_area);
        (Some(cols[0]), cols[1])
    } else {
        (None, main_area)
    };

    render_menu_bar(frame, menu_area, app);
    render_buffer_bar(frame, buffer_area, app);
    if let Some(ta) = tree_area {
        render_file_tree(frame, ta, app);
    }
    render_editor(frame, editor_area, app);
    render_status_bar(frame, status_area, app);

    // Submenu dropdown — drawn over the main content, below dialogs.
    render_submenu_dropdown(frame, menu_area, app);

    // Overlays — rendered on top.
    if let Some(ref dialog) = app.dialog.clone() {
        render_dialog(frame, area, dialog, app);
    } else if let CompletionState::Visible { ref items, selected, .. } = app.completion.clone() {
        render_completion(frame, editor_area, app, items, selected);
    }
}

fn render_buffer_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled(" ", Style::default().bg(C_DARK_GRAY))];
    spans.push(Span::styled(
        format!("[{}]", app.editor.display_name()),
        Style::default().bg(C_CYAN).fg(C_BLACK).add_modifier(Modifier::BOLD),
    ));
    for buffer in &app.buffers {
        spans.push(Span::styled(
            format!(" {} ", buffer.display_name()),
            Style::default().bg(C_DARK_GRAY).fg(C_WHITE),
        ));
    }
    spans.push(Span::styled(" ".repeat(area.width as usize), Style::default().bg(C_DARK_GRAY)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// Menu bar
// ---------------------------------------------------------------------------

const MENU_ITEMS: &[(&str, &str)] = &[
    ("F", "ile"),
    ("E", "dit"),
    ("S", "earch"),
    ("R", "un"),
    ("O", "ptions"),
    ("W", "indow"),
    ("H", "elp"),
];

fn render_menu_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();

    let (bg, fg) = (C_GRAY, C_BLACK);
    let (active_bg, active_fg) = (C_BLACK, C_YELLOW);

    // Leading space.
    spans.push(Span::styled(" ", Style::default().bg(bg).fg(fg)));

    for (idx, (hot, rest)) in MENU_ITEMS.iter().enumerate() {
        let is_active = app.focus == FocusArea::Menu && app.menu_index == idx;
        let (item_bg, item_fg, hot_fg) = if is_active {
            (active_bg, active_fg, active_fg)
        } else {
            (bg, fg, C_RED) // classic TP: hot-key is bright red on gray
        };

        // Underlined hotkey letter.
        spans.push(Span::styled(
            *hot,
            Style::default()
                .bg(item_bg)
                .fg(hot_fg)
                .add_modifier(Modifier::UNDERLINED),
        ));
        // Remaining label.
        spans.push(Span::styled(
            *rest,
            Style::default().bg(item_bg).fg(item_fg),
        ));
        // Separator space.
        spans.push(Span::styled("  ", Style::default().bg(bg).fg(fg)));
    }

    // Pad remainder.
    spans.push(Span::styled(
        " ".repeat(area.width as usize),
        Style::default().bg(bg).fg(fg),
    ));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Submenu dropdown
// ---------------------------------------------------------------------------

/// X offset (from the left edge of the menu bar) where menu item `idx` starts.
fn menu_item_x(idx: usize) -> u16 {
    let mut x = 1u16; // leading space
    for (i, (hot, rest)) in MENU_ITEMS.iter().enumerate() {
        if i == idx {
            return x;
        }
        x += (hot.len() + rest.len() + 2) as u16; // label + 2 separator spaces
    }
    x
}

/// Render the dropdown panel for the currently active submenu (if any).
fn render_submenu_dropdown(frame: &mut Frame, menu_area: Rect, app: &App) {
    if app.focus != FocusArea::Menu || !app.submenu_open {
        return;
    }
    let items: &[SubMenuItem] = SUBMENUS[app.menu_index];
    if items.is_empty() {
        return;
    }

    // Compute popup dimensions.
    let max_label = items
        .iter()
        .filter(|i| !i.is_separator())
        .map(|i| i.label.len())
        .max()
        .unwrap_or(8);
    let max_sc = items
        .iter()
        .filter(|i| !i.is_separator())
        .map(|i| i.shortcut.len())
        .max()
        .unwrap_or(0);
    // inner_width = 1 (left pad) + label + gap + shortcut + 1 (right pad)
    let inner_width = (1 + max_label + 2 + max_sc + 1).max(16) as u16;
    let popup_width = inner_width + 2; // +2 for the box border

    let x_raw = menu_area.x + menu_item_x(app.menu_index);
    let frame_width = frame.area().width;
    let x = x_raw.min(frame_width.saturating_sub(popup_width));

    let y = menu_area.y + menu_area.height;
    let frame_height = frame.area().height;
    let popup_height = (items.len() as u16 + 2).min(frame_height.saturating_sub(y));

    let popup = Rect { x, y, width: popup_width, height: popup_height };
    frame.render_widget(Clear, popup);

    let sep_line = "─".repeat(inner_width as usize);

    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            if item.is_separator() {
                return Line::from(Span::styled(
                    sep_line.clone(),
                    Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
                ));
            }
            let is_sel = idx == app.submenu_index;
            let (bg, fg) = if is_sel {
                (C_CYAN, C_BLACK)
            } else {
                (C_GRAY, C_BLACK)
            };
            // Layout: " label<gap>shortcut "
            let gap = (inner_width as usize)
                .saturating_sub(1 + item.label.len() + item.shortcut.len() + 1);
            let text = format!(
                " {}{}{} ",
                item.label,
                " ".repeat(gap),
                item.shortcut,
            );
            Line::from(Span::styled(text, Style::default().bg(bg).fg(fg)))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_DARK_GRAY))
        .style(Style::default().bg(C_GRAY));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

// ---------------------------------------------------------------------------
// File tree
// ---------------------------------------------------------------------------

fn render_file_tree(frame: &mut Frame, area: Rect, app: &mut App) {
    let visible_rows = (area.height as usize).saturating_sub(1).max(1);
    if app.tree_selected < app.tree_scroll {
        app.tree_scroll = app.tree_selected;
    } else if app.tree_selected >= app.tree_scroll + visible_rows {
        app.tree_scroll = app.tree_selected + 1 - visible_rows;
    }
    let items: Vec<ListItem> = app
        .tree_entries
        .iter()
        .enumerate()
        .skip(app.tree_scroll)
        .take(visible_rows)
        .map(|(i, entry)| {
            let is_selected = i == app.tree_selected;
            let style = if is_selected {
                Style::default().bg(C_CYAN).fg(C_BLACK)
            } else {
                Style::default().bg(C_BLUE_LIGHT).fg(C_WHITE)
            };
            ListItem::new(entry.display.clone()).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(" Files ", Style::default().bg(C_GRAY).fg(C_BLACK)))
        .borders(Borders::RIGHT | Borders::TOP)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_BLUE_LIGHT));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

fn render_editor(frame: &mut Frame, area: Rect, app: &mut App) {
    let visible_rows = area.height as usize;
    let gutter_width = 6usize.min(area.width as usize);
    let visible_cols = (area.width as usize).saturating_sub(gutter_width);
    let line_path = app
        .editor
        .file_path
        .as_ref()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
    let current_uri = app.editor.uri();
    let diagnostics = app.diagnostics.get(&current_uri).map(Vec::as_slice).unwrap_or(&[]);

    // Update scroll so cursor stays visible.
    let ed = &mut app.editor;
    if !app.preserve_editor_scroll {
        ed.scroll_to_cursor(visible_rows, visible_cols);
    }

    let scroll_row = app.editor.scroll_row;
    let scroll_col = app.editor.scroll_col;
    let cursor_row = app.editor.cursor_row;
    let cursor_col = app.editor.cursor_col;
    let lines = app.editor.lines();

    let mut text_lines: Vec<Line> = Vec::with_capacity(visible_rows);

    let mut in_block_comment = false;

    for row_idx in scroll_row..scroll_row + visible_rows {
        if row_idx >= lines.len() {
            // Empty line below buffer.
            text_lines.push(Line::from(Span::styled(
                format!("{}{}", " ".repeat(gutter_width), " ".repeat(visible_cols)),
                Style::default().bg(C_BLUE).fg(C_WHITE),
            )));
            continue;
        }

        let raw_line = &lines[row_idx];
        let (token_kinds, new_in_block) = highlight_line_for_language(
            raw_line,
            in_block_comment,
            app.current_language.map(|language| language.id),
        );
        in_block_comment = new_in_block;

        let is_cursor_line = row_idx == cursor_row;
        let line_bg = if is_cursor_line { C_BLUE_LIGHT } else { C_BLUE };

        let chars: Vec<char> = raw_line.chars().collect();
        let line_char_len = chars.len();
        let row_diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.range.start.line as usize <= row_idx
                    && row_idx <= diagnostic.range.end.line as usize
            })
            .collect();
        let line_severity = row_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.severity)
            .min_by_key(|severity| diagnostic_priority(*severity));

        // Build visible slice of the line with syntax highlighting.
        let mut spans: Vec<Span> = Vec::new();
        let display_len = visible_cols;

        let has_breakpoint = line_path
            .as_ref()
            .and_then(|p| app.breakpoints.get(p))
            .is_some_and(|rows| rows.contains(&row_idx));
        spans.push(Span::styled(
            line_severity.map(diagnostic_marker).unwrap_or(if has_breakpoint { '●' } else { ' ' }).to_string(),
            Style::default().bg(line_bg).fg(line_severity.map(diagnostic_color).unwrap_or(if has_breakpoint { C_RED } else { C_WHITE })),
        ));
        spans.push(Span::styled(
            format!("{:>4} ", row_idx + 1),
            Style::default().bg(line_bg).fg(C_CYAN),
        ));

        for col_idx in scroll_col..scroll_col + display_len {
            let is_cursor_col = is_cursor_line && col_idx == cursor_col;
            let is_selected = app.editor.is_selected(row_idx, col_idx);
            let diagnostic_severity = row_diagnostics
                .iter()
                .find(|diagnostic| diagnostic_covers_char(diagnostic, row_idx, col_idx, &chars))
                .map(|diagnostic| diagnostic.severity);

            if col_idx >= line_char_len {
                // Past end of line: render cursor block or blank.
                let (fg, bg) = if is_cursor_col && app.focus == FocusArea::Editor {
                    (C_BLUE, C_WHITE) // cursor at EOL
                } else if is_selected {
                    (C_BLACK, C_CYAN)
                } else if let Some(severity) = diagnostic_severity {
                    (C_BLACK, diagnostic_color(severity))
                } else {
                    (C_WHITE, line_bg)
                };
                let style = if diagnostic_severity.is_some() && !is_cursor_col && !is_selected {
                    Style::default().bg(bg).fg(fg).add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default().bg(bg).fg(fg)
                };
                spans.push(Span::styled(" ", style));
            } else {
                let ch = chars[col_idx];
                let kind = token_kinds.get(col_idx).copied().unwrap_or(TokenKind::Normal);

                let base_fg = match kind {
                    TokenKind::Keyword => C_KW,
                    TokenKind::Comment => C_COMMENT,
                    TokenKind::StringLiteral => C_STR,
                    TokenKind::Number => C_NUM,
                    TokenKind::Annotation => C_ANNOT,
                    TokenKind::Normal => C_GRAY,
                };

                let keyword_modifiers = if kind == TokenKind::Keyword {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                };
                let (fg, bg, mods) = if is_cursor_col && app.focus == FocusArea::Editor {
                    (C_BLUE, C_WHITE, Modifier::empty())
                } else if is_selected {
                    (C_BLACK, C_CYAN, Modifier::empty())
                } else if let Some(severity) = diagnostic_severity {
                    (
                        diagnostic_color(severity),
                        line_bg,
                        keyword_modifiers | Modifier::UNDERLINED,
                    )
                } else {
                    (base_fg, line_bg, keyword_modifiers)
                };

                let s: String = if ch == '\t' {
                    "    ".into()
                } else {
                    ch.to_string()
                };

                spans.push(Span::styled(
                    s,
                    Style::default().bg(bg).fg(fg).add_modifier(mods),
                ));
            }
        }

        text_lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(text_lines)
        .style(Style::default().bg(C_BLUE).fg(C_WHITE));

    frame.render_widget(paragraph, area);
}

fn diagnostic_priority(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Information => 2,
        DiagnosticSeverity::Hint => 3,
    }
}

fn diagnostic_marker(severity: DiagnosticSeverity) -> char {
    match severity {
        DiagnosticSeverity::Error => 'E',
        DiagnosticSeverity::Warning => 'W',
        DiagnosticSeverity::Information => 'i',
        DiagnosticSeverity::Hint => 'h',
    }
}

fn diagnostic_color(severity: DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Error => C_RED,
        DiagnosticSeverity::Warning => C_ORANGE,
        DiagnosticSeverity::Information => C_CYAN,
        DiagnosticSeverity::Hint => C_YELLOW,
    }
}

fn diagnostic_covers_char(
    diagnostic: &Diagnostic,
    row: usize,
    col: usize,
    line_chars: &[char],
) -> bool {
    let row = row as u32;
    if row < diagnostic.range.start.line || row > diagnostic.range.end.line {
        return false;
    }

    let start_col = if row == diagnostic.range.start.line {
        utf16_to_char_col(line_chars, diagnostic.range.start.character)
    } else {
        0
    };
    let mut end_col = if row == diagnostic.range.end.line {
        utf16_to_char_col(line_chars, diagnostic.range.end.character)
    } else {
        line_chars.len()
    };

    // Empty LSP ranges still need a visible mark at the reported position.
    if diagnostic.range.start.line == diagnostic.range.end.line
        && diagnostic.range.start.character == diagnostic.range.end.character
        && row == diagnostic.range.start.line
    {
        end_col = start_col.saturating_add(1);
    }

    (start_col..end_col).contains(&col)
}

fn utf16_to_char_col(chars: &[char], utf16_col: u32) -> usize {
    let mut units = 0u32;
    for (index, ch) in chars.iter().enumerate() {
        if units >= utf16_col {
            return index;
        }
        units = units.saturating_add(ch.len_utf16() as u32);
    }
    chars.len()
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let name = app.editor.display_name();
    let modified = if app.editor.modified { " [+]" } else { "" };
    let row = app.editor.cursor_row + 1;
    let col = app.editor.cursor_col + 1;

    // Language + LSP launch mode indicator (right of file name).
    let lang_tag = match app.current_language {
        Some(l) if app.lsp_launch_mode == "docker" => {
            format!(" [{}·🐳]", l.name)
        }
        Some(l) => format!(" [{}]", l.name),
        None => String::new(),
    };

    let (errors, warnings) = app
        .diagnostics
        .values()
        .flat_map(|v| v.iter())
        .fold((0usize, 0usize), |(e, w), d| {
            use crate::lsp::DiagnosticSeverity;
            match d.severity {
                DiagnosticSeverity::Error => (e + 1, w),
                DiagnosticSeverity::Warning => (e, w + 1),
                _ => (e, w),
            }
        });

    let diag_text = if errors > 0 {
        format!(" ✖ {} error{}", errors, if errors != 1 { "s" } else { "" })
    } else if warnings > 0 {
        format!(" ⚠ {} warning{}", warnings, if warnings != 1 { "s" } else { "" })
    } else if app.lsp_available {
        format!(" ✓ LSP({})", app.lsp_launch_mode)
    } else {
        " ○ no LSP".into()
    };

    let lsp_style = if errors > 0 {
        Style::default().bg(C_RED).fg(C_WHITE)
    } else if warnings > 0 {
        Style::default().bg(C_ORANGE).fg(C_BLACK)
    } else if app.lsp_available {
        Style::default().bg(C_GREEN).fg(C_BLACK)
    } else {
        Style::default().bg(C_DARK_GRAY).fg(C_WHITE)
    };

    let left = format!(" Ln {row}, Col {col}  {name}{modified}{lang_tag} ");
    let help = " F1=Help F2=Save F3=Open F10=Menu ";

    let left_len = left.chars().count();
    let diag_len = diag_text.chars().count();
    let help_len = help.len();
    let total_fixed = left_len + diag_len + help_len;
    let pad = (area.width as usize).saturating_sub(total_fixed);

    let spans = vec![
        Span::styled(left, Style::default().bg(C_CYAN).fg(C_BLACK)),
        Span::styled(diag_text, lsp_style),
        Span::styled(" ".repeat(pad), Style::default().bg(C_CYAN).fg(C_BLACK)),
        Span::styled(help, Style::default().bg(C_GRAY).fg(C_BLACK)),
    ];

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------------
// Completion popup
// ---------------------------------------------------------------------------

fn render_completion(
    frame: &mut Frame,
    editor_area: Rect,
    app: &App,
    items: &[crate::lsp::CompletionItem],
    selected: usize,
) {
    if items.is_empty() {
        return;
    }

    let max_rows = 8usize.min(items.len());
    let max_width = items
        .iter()
        .map(|i| i.label.len() + 6)
        .max()
        .unwrap_or(20)
        .min(40) as u16;

    // Position popup below the cursor.
    let cur_screen_row = (app.editor.cursor_row - app.editor.scroll_row) as u16;
    let cur_screen_col = 6 + (app.editor.cursor_col - app.editor.scroll_col) as u16;

    let popup_row = (editor_area.y + cur_screen_row + 1).min(
        editor_area.y + editor_area.height - max_rows as u16 - 2,
    );
    let popup_col =
        (editor_area.x + cur_screen_col).min(editor_area.x + editor_area.width - max_width - 2);

    let popup_area = Rect {
        x: popup_col,
        y: popup_row,
        width: max_width + 2,
        height: max_rows as u16 + 2,
    };

    frame.render_widget(Clear, popup_area);

    let scroll_offset = if selected >= max_rows {
        selected - max_rows + 1
    } else {
        0
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(max_rows)
        .map(|(i, item)| {
            let is_sel = i == selected;
            let icon = item.kind.icon();
            let label = format!("[{icon}] {}", item.label);
            let style = if is_sel {
                Style::default().bg(C_CYAN).fg(C_BLACK)
            } else {
                Style::default().bg(C_DARK_GRAY).fg(C_WHITE)
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " Completions ",
            Style::default().bg(C_GRAY).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_DARK_GRAY));

    let list = List::new(list_items).block(block);
    frame.render_widget(list, popup_area);
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

fn render_dialog(frame: &mut Frame, area: Rect, dialog: &Dialog, app: &App) {
    match dialog {
        Dialog::Help => render_help_dialog(frame, area),
        Dialog::About => render_about_dialog(frame, area, app),
        Dialog::OpenFile { input, .. } => render_open_file_dialog(frame, area, input),
        Dialog::SaveAs { input } => render_save_as_dialog(frame, area, input),
        Dialog::Find { input, .. } => render_find_dialog(frame, area, input),
        Dialog::Replace { find, replace_with, focus_replace, .. } => {
            render_replace_dialog(frame, area, find, replace_with, *focus_replace);
        }
        Dialog::GoToLine { input } => render_goto_line_dialog(frame, area, input),
        Dialog::Message { text, .. } => {
            render_action_dialog(frame, area, " Message ", text, &["OK"]);
        }
        Dialog::Hover { text, scroll } => render_hover_dialog(frame, area, text, *scroll),
        Dialog::DockerApproval { image } => render_action_dialog(
            frame,
            area,
            " Docker LSP ",
            &format!("Allow BlueIDE to pull {image} if needed and run its language server?"),
            &["Approve", "Cancel"],
        ),
        Dialog::ConfirmDiscard { .. } => render_action_dialog(
            frame,
            area,
            " Unsaved Changes ",
            "Unsaved changes. Press Y to discard, N or Esc to cancel.",
            &["Discard", "Cancel"],
        ),
        Dialog::BuildOutput { title, lines, diagnostics, selected } => {
            render_build_output(frame, area, title, lines, diagnostics, *selected);
        }
        Dialog::SettingsInput { kind, input } => {
            let (title, label) = match kind {
                SettingKind::Parameters => (" Run Parameters ", "Args: "),
                SettingKind::Compiler(_) => (" Compiler Command ", "Command: "),
            };
            render_input_dialog(frame, area, title, input, label, 60, &["Save", "Cancel"]);
        }
    }
}

fn render_build_output(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    output: &[String],
    diagnostics: &[BuildDiagnostic],
    selected: usize,
) {
    let popup = dialog_centered(area, area.width.saturating_sub(8), area.height.saturating_sub(4));
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!(" {title} · ↑/↓ select or scroll · Enter jump · Esc close "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_CYAN))
        .style(Style::default().bg(C_BLUE).fg(C_WHITE));
    let body_height = popup.height.saturating_sub(3) as usize;
    let selected_line = diagnostics.get(selected).map(|d| d.output_line).unwrap_or(0);
    let start = if diagnostics.is_empty() {
        selected.min(output.len().saturating_sub(body_height))
    } else {
        selected_line.saturating_sub(body_height.saturating_sub(1))
    };
    let mut lines: Vec<Line> = output
        .iter()
        .enumerate()
        .skip(start)
        .take(body_height)
        .map(|(idx, line)| {
            let is_selected = diagnostics.get(selected).is_some_and(|d| d.output_line == idx);
            let style = if is_selected {
                Style::default().bg(C_YELLOW).fg(C_BLACK)
            } else if diagnostics.iter().any(|d| d.output_line == idx) {
                Style::default().fg(C_RED)
            } else {
                Style::default().fg(C_WHITE)
            };
            Line::from(Span::styled(line.as_str(), style))
        })
        .collect();
    if output.is_empty() {
        lines.push(Line::from("(no output)"));
    }
    lines.resize_with(body_height, || Line::from(""));
    let close_width = " Close ".chars().count();
    let left_pad = popup.width.saturating_sub(2) as usize;
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(left_pad.saturating_sub(close_width) / 2)),
        Span::styled(
            " Close ",
            Style::default().bg(C_CYAN).fg(C_BLACK).add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn dialog_centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}

fn render_help_dialog(frame: &mut Frame, area: Rect) {
    const HELP: &[HelpLine] = &[
        HelpLine { key: "F1",                  action: "Help" },
        HelpLine { key: "F2 / Ctrl+S",         action: "Save" },
        HelpLine { key: "F3 / Ctrl+O",         action: "Open file" },
        HelpLine { key: "F5",                  action: "Toggle file browser" },
        HelpLine { key: "F6",                  action: "Hover info" },
        HelpLine { key: "F8 / Shift+F8",       action: "Breakpoint / GDB debugger" },
        HelpLine { key: "F9 / Ctrl+F9",        action: "Compile / Run" },
        HelpLine { key: "F10",                 action: "Activate menu bar" },
        HelpLine { key: "F12",                 action: "Go to definition" },
        HelpLine { key: "Alt+F/E/S/R/O/W/H",  action: "Open menu directly" },
        HelpLine { key: "Ctrl+Q / Alt+F4",     action: "Quit" },
        HelpLine { key: "Ctrl+Z / Ctrl+Y",     action: "Undo / Redo" },
        HelpLine { key: "Ctrl+X / Ins / Shift+Ins", action: "Cut / Copy / Paste" },
        HelpLine { key: "Shift+Enter / Ctrl+J", action: "Insert blank line below" },
        HelpLine { key: "Shift+Arrow / Ctrl+A", action: "Select / Select all" },
        HelpLine { key: "Ctrl+Tab / Ctrl+PgUp/Dn", action: "Cycle open buffers" },
        HelpLine { key: "Ctrl+1..9 / Mouse",   action: "Jump to buffer / click UI" },
        HelpLine { key: "Ctrl+F / Ctrl+L / H", action: "Find / Again / Replace" },
        HelpLine { key: "Ctrl+G",              action: "Go to Line" },
        HelpLine { key: "Ctrl+Space",          action: "Code completion" },
        HelpLine { key: "ESC",                 action: "Close dialog / menu" },
    ];

    let width = 52u16;
    let height = HELP.len() as u16 + 4;
    let popup = dialog_centered(area, width, height);
    frame.render_widget(Clear, popup);

    let lines: Vec<Line> = HELP
        .iter()
        .map(|hl| {
            Line::from(vec![
                Span::styled(
                    format!("  {:20}", hl.key),
                    Style::default().bg(C_GRAY).fg(C_BLUE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", hl.action),
                    Style::default().bg(C_GRAY).fg(C_BLACK),
                ),
            ])
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " BlueIDE Help  (press any key to close) ",
            Style::default().bg(C_CYAN).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

fn render_open_file_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Open File ", input, "Path: ", 50, &["Open", "Cancel"]);
}

fn render_save_as_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Save As ", input, "Path: ", 50, &["Save", "Cancel"]);
}

fn render_find_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Find ", input, "Search: ", 40, &["Find", "Cancel"]);
}

fn render_input_dialog(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    input: &str,
    label: &str,
    width: u16,
    buttons: &[&str],
) {
    let popup = dialog_centered(area, width, 6);
    frame.render_widget(Clear, popup);

    let display = format!("{label}{input}_");
    let inner_width = popup.width.saturating_sub(2) as usize;
    let button_width: usize = buttons.iter().map(|label| label.chars().count() + 2).sum::<usize>()
        + buttons.len().saturating_sub(1) * 2;
    let mut button_spans = vec![Span::raw(" ".repeat(inner_width.saturating_sub(button_width) / 2))];
    for (index, label) in buttons.iter().enumerate() {
        if index > 0 { button_spans.push(Span::raw("  ")); }
        button_spans.push(Span::styled(
            format!(" {label} "),
            Style::default().bg(C_CYAN).fg(C_BLACK).add_modifier(Modifier::BOLD),
        ));
    }
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {display}"),
            Style::default().bg(C_GRAY).fg(C_BLACK),
        )),
        Line::from(""),
        Line::from(button_spans),
    ];

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().bg(C_CYAN).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

fn render_action_dialog(frame: &mut Frame, area: Rect, title: &str, text: &str, buttons: &[&str]) {
    let button_width: usize = buttons.iter().map(|label| label.chars().count() + 2).sum::<usize>()
        + buttons.len().saturating_sub(1) * 2;
    let width = area.width.saturating_sub(4).min(80).max(30);
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let wrapped_lines: usize = text
        .lines()
        .map(|line| line.chars().count().max(1).div_ceil(inner_width))
        .sum();
    let height = (wrapped_lines.max(1) as u16 + 5)
        .min(area.height.saturating_sub(2))
        .max(5);
    let popup = dialog_centered(area, width.min(area.width), height.min(area.height));
    frame.render_widget(Clear, popup);

    let mut button_spans = vec![Span::raw(" ".repeat((inner_width.saturating_sub(button_width)) / 2))];
    for (index, label) in buttons.iter().enumerate() {
        if index > 0 {
            button_spans.push(Span::raw("  "));
        }
        button_spans.push(Span::styled(
            format!(" {label} "),
            Style::default().bg(C_CYAN).fg(C_BLACK).add_modifier(Modifier::BOLD),
        ));
    }
    let lines = vec![
        Line::from(Span::styled(text, Style::default().bg(C_GRAY).fg(C_BLACK))),
        Line::from(""),
        Line::from(""),
        Line::from(button_spans),
    ];

    let footer = if buttons.len() == 1 {
        " Enter / Esc close "
    } else {
        " Enter/Y confirm  Esc/N cancel "
    };
    let block = Block::default()
        .title(Span::styled(title, Style::default().bg(C_CYAN).fg(C_BLACK)))
        .title_bottom(Span::styled(
            footer,
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).block(block);
    frame.render_widget(para, popup);
}

pub(crate) fn dialog_button_at(dialog: &Dialog, area: Rect, x: u16, y: u16) -> Option<usize> {
    let (buttons, width, height, text) = match dialog {
        Dialog::Message { text } => (&["OK"][..], area.width.saturating_sub(4).min(80).max(30), 0, Some(text.clone())),
        Dialog::DockerApproval { image } => (
            &["Approve", "Cancel"][..],
            area.width.saturating_sub(4).min(80).max(30),
            0,
            Some(format!("Allow BlueIDE to pull {image} if needed and run its language server?")),
        ),
        Dialog::ConfirmDiscard { .. } => (
            &["Discard", "Cancel"][..],
            area.width.saturating_sub(4).min(80).max(30),
            0,
            Some("Unsaved changes. Press Y to discard, N or Esc to cancel.".into()),
        ),
        Dialog::OpenFile { .. } => (&["Open", "Cancel"][..], 50, 6, None),
        Dialog::SaveAs { .. } => (&["Save", "Cancel"][..], 50, 6, None),
        Dialog::Find { .. } => (&["Find", "Cancel"][..], 40, 6, None),
        Dialog::GoToLine { .. } => (&["Go", "Cancel"][..], 30, 6, None),
        Dialog::SettingsInput { .. } => (&["Save", "Cancel"][..], 60, 6, None),
        Dialog::BuildOutput { .. } => (
            &["Close"][..],
            area.width.saturating_sub(8),
            area.height.saturating_sub(4),
            None,
        ),
        _ => return None,
    };
    let button_width: usize = buttons.iter().map(|label| label.chars().count() + 2).sum::<usize>()
        + buttons.len().saturating_sub(1) * 2;
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let height = if let Some(text) = text {
        let wrapped_lines: usize = text
            .lines()
            .map(|line| line.chars().count().max(1).div_ceil(inner_width))
            .sum();
        (wrapped_lines.max(1) as u16 + 5)
            .min(area.height.saturating_sub(2))
            .max(5)
    } else {
        height
    };
    let popup = dialog_centered(area, width.min(area.width), height.min(area.height));
    let button_row = popup.y.saturating_add(popup.height.saturating_sub(2));
    if y != button_row {
        return None;
    }
    let mut button_x = popup.x.saturating_add(1)
        .saturating_add((inner_width.saturating_sub(button_width) / 2) as u16);
    for (index, label) in buttons.iter().enumerate() {
        let end_x = button_x.saturating_add(label.chars().count() as u16 + 2);
        if (button_x..end_x).contains(&x) {
            return Some(index);
        }
        button_x = end_x.saturating_add(2);
    }
    None
}

fn render_hover_dialog(frame: &mut Frame, area: Rect, text: &str, scroll: u16) {
    let popup = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4).max(1),
        height: area.height.saturating_sub(2).max(1),
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(Span::styled(" Hover Info ", Style::default().bg(C_CYAN).fg(C_BLACK)))
        .title_bottom(Span::styled(
            " ↑/↓ scroll  PgUp/PgDn page  Home/End  Esc close ",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let inner_height = popup.height.saturating_sub(2);
    let inner_width = popup.width.saturating_sub(2);
    let line_width = inner_width.max(1) as usize;
    let visual_lines: usize = text
        .lines()
        .map(|line| line.chars().count().max(1).div_ceil(line_width))
        .sum();
    let max_scroll = visual_lines
        .saturating_sub(inner_height as usize)
        .min(u16::MAX as usize) as u16;
    let para = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((scroll.min(max_scroll), 0))
        .block(block);
    frame.render_widget(para, popup);
}

fn render_replace_dialog(
    frame: &mut Frame,
    area: Rect,
    find: &str,
    replace_with: &str,
    focus_replace: bool,
) {
    let width = 52u16;
    let popup = dialog_centered(area, width, 7);
    frame.render_widget(Clear, popup);

    let find_cursor = if !focus_replace { "_" } else { "" };
    let repl_cursor = if focus_replace { "_" } else { "" };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Search:  ", Style::default().bg(C_GRAY).fg(C_DARK_GRAY)),
            Span::styled(
                format!("{find}{find_cursor}"),
                Style::default().bg(C_GRAY).fg(C_BLACK),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Replace: ", Style::default().bg(C_GRAY).fg(C_DARK_GRAY)),
            Span::styled(
                format!("{replace_with}{repl_cursor}"),
                Style::default().bg(C_GRAY).fg(C_BLACK),
            ),
        ]),
        Line::from(Span::styled(
            "  [Tab] switch  [Enter] replace  [Esc] cancel",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
    ];

    let block = Block::default()
        .title(Span::styled(" Replace ", Style::default().bg(C_CYAN).fg(C_BLACK)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

fn render_goto_line_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Go to Line ", input, "Line: ", 30, &["Go", "Cancel"]);
}

fn render_about_dialog(frame: &mut Frame, area: Rect, app: &App) {
    let lang_line = match app.current_language {
        Some(l) if app.lsp_launch_mode == "docker" => {
            format!("  Language: {} · LSP via Docker ({})", l.name, l.docker_image)
        }
        Some(l) if app.lsp_launch_mode == "native" => {
            format!("  Language: {} · LSP native ({})", l.name, l.native_cmd)
        }
        Some(l) => format!("  Language: {} · no LSP", l.name),
        None => "  Language: unknown · no LSP".into(),
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  BlueIDE — Multi-language Terminal IDE",
            Style::default()
                .bg(C_GRAY)
                .fg(C_BLACK)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Powered by: Ratatui · Tokio · Crossterm",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
        Line::from(Span::styled(
            lang_line,
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
    ];

    let width = 58u16;
    let height = lines.len() as u16 + 2;
    let popup = dialog_centered(area, width, height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            " About BlueIDE ",
            Style::default().bg(C_CYAN).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}
