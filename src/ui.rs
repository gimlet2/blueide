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
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, CompletionState, Dialog, FocusArea, HelpLine, SubMenuItem, SUBMENUS};
use crate::editor::{highlight_line, TokenKind};

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
/// Keyword color (bright cyan in editor).
pub const C_KW: Color = Color::Rgb(85, 255, 255);
/// String literal color (bright green).
pub const C_STR: Color = Color::Rgb(85, 255, 85);
/// Comment color (dark gray / italic-ish).
pub const C_COMMENT: Color = Color::Rgb(85, 170, 85);
/// Number color.
pub const C_NUM: Color = Color::Rgb(255, 170, 85);
/// Annotation color.
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
            Constraint::Min(0),    // main area
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let menu_area = rows[0];
    let main_area = rows[1];
    let status_area = rows[2];

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

fn render_file_tree(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .tree_entries
        .iter()
        .enumerate()
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
        .borders(Borders::RIGHT)
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
    let visible_cols = area.width as usize;

    // Update scroll so cursor stays visible.
    let ed = &mut app.editor;
    ed.scroll_to_cursor(visible_rows, visible_cols);

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
                " ".repeat(visible_cols),
                Style::default().bg(C_BLUE).fg(C_WHITE),
            )));
            continue;
        }

        let raw_line = &lines[row_idx];
        let (token_kinds, new_in_block) = highlight_line(raw_line, in_block_comment);
        in_block_comment = new_in_block;

        let is_cursor_line = row_idx == cursor_row;
        let line_bg = if is_cursor_line { C_BLUE_LIGHT } else { C_BLUE };

        let chars: Vec<char> = raw_line.chars().collect();
        let line_char_len = chars.len();

        // Build visible slice of the line with syntax highlighting.
        let mut spans: Vec<Span> = Vec::new();
        let display_len = visible_cols;

        for col_idx in scroll_col..scroll_col + display_len {
            let is_cursor_col = is_cursor_line && col_idx == cursor_col;

            if col_idx >= line_char_len {
                // Past end of line: render cursor block or blank.
                let (fg, bg) = if is_cursor_col && app.focus == FocusArea::Editor {
                    (C_BLUE, C_WHITE) // cursor at EOL
                } else {
                    (C_WHITE, line_bg)
                };
                spans.push(Span::styled(" ", Style::default().bg(bg).fg(fg)));
            } else {
                let ch = chars[col_idx];
                let kind = token_kinds.get(col_idx).copied().unwrap_or(TokenKind::Normal);

                let base_fg = match kind {
                    TokenKind::Keyword => C_KW,
                    TokenKind::Comment => C_COMMENT,
                    TokenKind::StringLiteral => C_STR,
                    TokenKind::Number => C_NUM,
                    TokenKind::Annotation => C_ANNOT,
                    TokenKind::Normal => C_WHITE,
                };

                let (fg, bg, mods) = if is_cursor_col && app.focus == FocusArea::Editor {
                    (C_BLUE, C_WHITE, Modifier::empty())
                } else {
                    (base_fg, line_bg, Modifier::empty())
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

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let name = app.editor.display_name();
    let modified = if app.editor.modified { " [+]" } else { "" };
    let row = app.editor.cursor_row + 1;
    let col = app.editor.cursor_col + 1;

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
        " ✓ LSP".into()
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

    let left = format!(" Ln {row}, Col {col}  {name}{modified} ");
    let help = " F1=Help F2=Save F3=Open F10=Menu ";

    let left_len = left.len();
    let diag_len = diag_text.len();
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
    let cur_screen_col = (app.editor.cursor_col - app.editor.scroll_col) as u16;

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

fn render_dialog(frame: &mut Frame, area: Rect, dialog: &Dialog, _app: &App) {
    match dialog {
        Dialog::Help => render_help_dialog(frame, area),
        Dialog::About => render_about_dialog(frame, area),
        Dialog::OpenFile { input, .. } => render_open_file_dialog(frame, area, input),
        Dialog::SaveAs { input } => render_save_as_dialog(frame, area, input),
        Dialog::Find { input, .. } => render_find_dialog(frame, area, input),
        Dialog::Replace { find, replace_with, focus_replace, .. } => {
            render_replace_dialog(frame, area, find, replace_with, *focus_replace);
        }
        Dialog::GoToLine { input } => render_goto_line_dialog(frame, area, input),
        Dialog::Message { text, .. } => render_message_dialog(frame, area, text),
        Dialog::Hover { text } => render_hover_dialog(frame, area, text),
    }
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
        HelpLine { key: "F5",                  action: "Run / Compile" },
        HelpLine { key: "F9",                  action: "Compile" },
        HelpLine { key: "F10",                 action: "Activate menu bar" },
        HelpLine { key: "F12",                 action: "Go to definition" },
        HelpLine { key: "Alt+F/E/S/R/O/W/H",  action: "Open menu directly" },
        HelpLine { key: "Ctrl+Q / Alt+F4",     action: "Quit" },
        HelpLine { key: "Ctrl+Z / Ctrl+Y",     action: "Undo / Redo" },
        HelpLine { key: "Ctrl+X",              action: "Cut line" },
        HelpLine { key: "Ctrl+Ins",            action: "Copy line" },
        HelpLine { key: "Shift+Ins",           action: "Paste" },
        HelpLine { key: "Ctrl+F",              action: "Find" },
        HelpLine { key: "Ctrl+L",              action: "Find Again" },
        HelpLine { key: "Ctrl+H",              action: "Replace" },
        HelpLine { key: "Ctrl+G",              action: "Go to Line" },
        HelpLine { key: "Ctrl+Space",          action: "Code completion" },
        HelpLine { key: "Ctrl+B",              action: "Toggle file browser" },
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
    render_input_dialog(frame, area, " Open File ", input, "Path: ", 50);
}

fn render_save_as_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Save As ", input, "Path: ", 50);
}

fn render_find_dialog(frame: &mut Frame, area: Rect, input: &str) {
    render_input_dialog(frame, area, " Find ", input, "Search: ", 40);
}

fn render_input_dialog(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    input: &str,
    label: &str,
    width: u16,
) {
    let popup = dialog_centered(area, width, 5);
    frame.render_widget(Clear, popup);

    let display = format!("{label}{input}_");
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {display}"),
            Style::default().bg(C_GRAY).fg(C_BLACK),
        )),
        Line::from(Span::styled(
            "  [Enter] confirm   [Esc] cancel",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
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

fn render_message_dialog(frame: &mut Frame, area: Rect, text: &str) {
    let width = (text.len() as u16 + 8).max(30).min(60);
    let popup = dialog_centered(area, width, 5);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {text}"),
            Style::default().bg(C_GRAY).fg(C_BLACK),
        )),
        Line::from(Span::styled(
            "  [Enter / Esc] close",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
    ];

    let block = Block::default()
        .title(Span::styled(
            " Message ",
            Style::default().bg(C_CYAN).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

fn render_hover_dialog(frame: &mut Frame, area: Rect, text: &str) {
    let first_line = text.lines().next().unwrap_or(text);
    let width = (first_line.len() as u16 + 8).min(70).max(30);
    let height = (text.lines().count() as u16 + 4).min(20);
    let popup = dialog_centered(area, width, height);
    frame.render_widget(Clear, popup);

    let lines: Vec<Line> = std::iter::once(Line::from(""))
        .chain(text.lines().map(|l| {
            Line::from(Span::styled(
                format!("  {l}"),
                Style::default().bg(C_GRAY).fg(C_BLACK),
            ))
        }))
        .chain(std::iter::once(Line::from(Span::styled(
            "  [Esc] close",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        ))))
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " Hover Info ",
            Style::default().bg(C_CYAN).fg(C_BLACK),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_GRAY))
        .style(Style::default().bg(C_GRAY));

    let para = Paragraph::new(lines).block(block);
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
    render_input_dialog(frame, area, " Go to Line ", input, "Line: ", 30);
}

fn render_about_dialog(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  BlueIDE — Turbo Pascal-style Kotlin IDE",
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
            "  LSP support: Kotlin Language Server",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().bg(C_GRAY).fg(C_DARK_GRAY),
        )),
    ];

    let width = 48u16;
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
