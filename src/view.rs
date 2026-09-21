//! Pure Ratatui renderer: no terminal input, filesystem, clock or launches.
use crate::{
    app::{Action, App, Focus, ScrollTarget},
    cells::{clip, input_cursor, input_window, width},
    theme::Theme,
};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

// Draw into a buffer shared by the terminal renderer and deterministic tests.
struct Canvas<'a> {
    buffer: &'a mut Buffer,
    theme: Theme,
    cursor: Option<Position>,
}
impl Canvas<'_> {
    fn area(&self) -> Rect {
        self.buffer.area
    }
    fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        widget.render(area, self.buffer);
    }
    fn set_cursor_position(&mut self, position: impl Into<Position>) {
        self.cursor = Some(position.into());
    }
}

fn row(f: &mut Canvas, area: Rect, text: &str, style: Style) {
    if area.width > 0 && area.height > 0 {
        f.render_widget(
            Paragraph::new(clip(text, area.width as usize)).style(style),
            area,
        );
    }
}
fn at(area: Rect, y: u16, height: u16) -> Rect {
    Rect::new(area.x, y, area.width, height).intersection(area)
}
fn pair(f: &mut Canvas, area: Rect, left: &str, right: &str, style: Style) {
    let theme = f.theme;
    let rw = (width(right) as u16).min(area.width);
    row(
        f,
        Rect::new(area.right() - rw, area.y, rw, 1),
        right,
        theme.muted(),
    );
    row(
        f,
        Rect::new(area.x, area.y, area.width.saturating_sub(rw + 1), 1),
        left,
        style,
    );
}
fn section(f: &mut Canvas, a: Rect, label: &str, hint: &str, active: bool) {
    let theme = f.theme;
    pair(
        f,
        a,
        label,
        hint,
        if active {
            theme.accent()
        } else {
            theme.muted()
        },
    );
}

fn controls(f: &mut Canvas, hits: &mut HitMap, area: Rect, items: &[(&str, Action)]) {
    let theme = f.theme;
    let mut x = area.x;
    for (i, (label, action)) in items.iter().enumerate() {
        if i > 0 {
            row(
                f,
                Rect::new(x, area.y, 3, 1).intersection(area),
                " · ",
                theme.muted().bg(theme.surface),
            );
            x += 3;
        }
        let w = width(label) as u16;
        if x + w > area.right() || area.height == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, w, 1);
        row(f, rect, label, theme.accent().bg(theme.surface));
        hits.add(rect, action.clone());
        x += w;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Pointer {
    Click,
    ScrollUp,
    ScrollDown,
}
#[derive(Debug, Default)]
pub struct HitMap {
    area: Rect,
    regions: Vec<(Rect, Action)>,
    wheels: Vec<(Rect, ScrollTarget)>,
}
impl HitMap {
    pub fn action(&self, pointer: Pointer, x: u16, y: u16, current: Rect) -> Option<Action> {
        if self.area != current {
            return None;
        }
        if !matches!(pointer, Pointer::Click) {
            return self
                .wheels
                .iter()
                .rev()
                .find(|(r, _)| r.contains((x, y).into()))
                .map(|(_, target)| {
                    Action::Scroll(*target, matches!(pointer, Pointer::ScrollDown))
                });
        }
        self.regions
            .iter()
            .rev()
            .find(|(r, _)| r.contains((x, y).into()))
            .map(|(_, a)| a.clone())
    }
    fn add(&mut self, area: Rect, action: Action) {
        let area = area.intersection(self.area);
        if !area.is_empty() {
            self.regions.push((area, action));
        }
    }
}
pub fn render_with_hits(f: &mut Frame, app: &App) -> HitMap {
    let (hits, cursor) = render_buffer(f.buffer_mut(), app);
    if let Some(cursor) = cursor {
        f.set_cursor_position(cursor);
    }
    hits
}
/// Draw a complete dashboard into a caller-owned, cleared buffer.
pub fn render_buffer(buffer: &mut Buffer, app: &App) -> (HitMap, Option<Position>) {
    let mut hits = HitMap {
        area: buffer.area,
        ..HitMap::default()
    };
    let mut canvas = Canvas {
        buffer,
        theme: app.theme,
        cursor: None,
    };
    render_ui(&mut canvas, app, &mut hits);
    (hits, canvas.cursor)
}
pub fn render(f: &mut Frame, app: &App) {
    render_with_hits(f, app);
}
fn render_ui(f: &mut Canvas, app: &App, hits: &mut HitMap) {
    let theme = f.theme;
    let viewport = f.area();
    f.render_widget(Block::default().style(theme.base()), viewport);
    let width = viewport.width.min(160);
    let full = Rect::new(
        viewport.x + (viewport.width - width) / 2,
        viewport.y,
        width,
        viewport.height,
    );
    if full.width < 40 || full.height < 10 {
        f.render_widget(
            Paragraph::new("Resize to at least 40 × 10.\nNo launch at this size.\nCtrl+Q quit")
                .style(theme.accent())
                .wrap(Wrap { trim: false }),
            full,
        );
        return;
    }
    let margin = if full.width >= 80 { 2 } else { 1 };
    let area = Rect::new(
        full.x + margin,
        full.y,
        full.width - margin * 2,
        full.height,
    );
    if app.help {
        help(f, app, area, hits);
        return;
    }
    dashboard(f, app, area, hits);
}

fn dashboard(f: &mut Canvas, app: &App, area: Rect, hits: &mut HitMap) {
    let theme = f.theme;
    let short = area.height < 18;
    let roomy = area.height >= 30;
    let narrow = area.width < 60;
    pair(
        f,
        at(area, area.y, 1),
        "›_ Launchpad",
        match (app.simulate_launch, narrow) {
            (false, false) => "HOME / replace this pane",
            (false, true) => "REPLACE PANE",
            (true, false) => "HOME / launch suppressed",
            (true, true) => "SUPPRESSED",
        },
        theme.accent().add_modifier(Modifier::BOLD),
    );
    let mut y = area.y + if roomy { 2 } else { 1 };
    section(
        f,
        at(area, y, 1),
        "01 Directory",
        "Ctrl+P",
        app.focus == Focus::Path,
    );
    y += 1;
    let path_height = if short { 1 } else { 3 };
    let input = at(area, y, path_height);
    let inner = if short {
        f.render_widget(
            Block::default().style(theme.base().bg(theme.surface)),
            input,
        );
        input
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if app.focus == Focus::Path {
                theme.accent()
            } else {
                theme.base().fg(theme.border)
            })
            .style(theme.base().bg(theme.surface));
        let inner = block.inner(input);
        f.render_widget(block, input);
        inner
    };
    row(
        f,
        Rect::new(inner.x + 1, inner.y, 2, 1),
        "›",
        theme.accent().bg(theme.surface),
    );
    let text_area = Rect::new(inner.x + 3, inner.y, inner.width.saturating_sub(4), 1);
    for x in input.x..input.right() {
        let cursor = input_cursor(
            &app.editor.text,
            app.editor.cursor,
            text_area.width as usize,
            x.saturating_sub(text_area.x) as usize,
        );
        hits.add(
            Rect::new(x, input.y, 1, input.height),
            Action::PathCursor(cursor),
        );
    }
    let (visible, caret) = input_window(
        &app.editor.text,
        app.editor.cursor,
        text_area.width as usize,
    );
    row(
        f,
        text_area,
        &visible,
        theme.base().bg(theme.surface).add_modifier(Modifier::BOLD),
    );
    if app.focus == Focus::Path && text_area.width > 0 {
        f.set_cursor_position((text_area.x + caret as u16, text_area.y));
    }
    y += path_height;
    let suggestion_rows: u16 = if short && app.focus == Focus::History && app.message.is_some() {
        0
    } else if short {
        1
    } else if roomy {
        4
    } else {
        3
    };
    suggestions(
        f,
        app,
        Rect::new(area.x, y, area.width, suggestion_rows),
        hits,
    );
    y += suggestion_rows + u16::from(roomy);
    let tools_heading = y;
    section(
        f,
        at(area, y, 1),
        "02 Launch with",
        "Ctrl+T",
        app.focus == Focus::Tools,
    );
    y += 1;
    let tools = app.visible_tools();
    let selected = tools.iter().position(|&t| t == app.tool).unwrap_or(0);
    let mut positions = Vec::new();
    let (mut column, mut tool_row) = (0u16, 0u16);
    for &tool in &tools {
        let label = clip(&app.tool_label(tool), area.width.saturating_sub(4) as usize);
        let label = if app.tool == tool {
            format!("[ {label} ]")
        } else {
            format!("  {label}  ")
        };
        let w = width(&label) as u16;
        if column > 0 && column + w > area.width {
            column = 0;
            tool_row += 1;
        }
        positions.push((tool_row, column, w, label));
        column += w + 1;
    }
    let max_rows = if short
        && (app.focus == Focus::History
            || app.message.is_some()
            || app.config_errors().next().is_some())
    {
        1
    } else {
        2
    };
    let start = positions[selected].0.saturating_sub(max_rows - 1);
    let visible_rows = (tool_row + 1).min(max_rows);
    let mut x = area.x;
    for (&tool, (r, col, w, label)) in tools.iter().zip(&positions) {
        if *r < start || *r >= start + visible_rows {
            continue;
        }
        let row_y = y + r - start;
        x = area.x + col;
        let style = if app.tool == tool {
            if app.focus == Focus::Tools {
                theme
                    .base()
                    .fg(theme.on_accent)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme.accent()
            }
        } else {
            theme.muted()
        };
        row(f, Rect::new(x, row_y, *w, 1), label, style);
        hits.add(Rect::new(x, row_y, *w, 1), Action::SelectTool(tool));
        x += w + 1;
    }
    y += visible_rows - 1;
    if tool_row + 1 > visible_rows {
        let heading = Rect::new(area.right() - 22, tools_heading, 14, 1);
        row(
            f,
            heading,
            &format!("‹ {:02}/{:02} ›", selected + 1, tools.len()),
            theme.accent(),
        );
        hits.add(
            Rect::new(heading.x, heading.y, 2, 1),
            Action::SelectTool(tools[(selected + tools.len() - 1) % tools.len()]),
        );
        hits.add(
            Rect::new(heading.x + 8, heading.y, 2, 1),
            Action::SelectTool(tools[(selected + 1) % tools.len()]),
        );
    }
    if !narrow && x + 12 <= area.right() {
        row(
            f,
            Rect::new(area.right() - 12, y, 12, 1),
            " Enter ↵ ",
            theme.base().fg(theme.on_accent).bg(theme.accent),
        );
        hits.add(Rect::new(area.right() - 12, y, 12, 1), Action::LaunchForm);
    } else {
        controls(
            f,
            hits,
            Rect::new(area.right() - 7, tools_heading, 7, 1),
            &[("Enter ↵", Action::LaunchForm)],
        );
    }
    y += 1;
    let config_error = app.config_errors().next().map(|e| {
        format!(
            "Config error: {e} (F1: all {} errors)",
            app.config_errors().count()
        )
    });
    let status_height = if let Some(message) = app.message.as_ref().or(config_error.as_ref()) {
        let max_height = if short && app.focus == Focus::History {
            area.bottom().saturating_sub(y + 3).clamp(1, 3)
        } else {
            3
        };
        let h = (width(message).div_ceil(area.width as usize) as u16).clamp(1, max_height);
        f.render_widget(
            Paragraph::new(message.as_str())
                .style(theme.base().fg(theme.error))
                .wrap(Wrap { trim: false }),
            at(area, y, h),
        );
        h
    } else if short {
        0
    } else {
        row(f, at(area, y, 1), &app.search_status, theme.muted());
        1
    };
    y += status_height;
    let footer_y = area.bottom() - 1;
    hits.wheels.push((
        Rect::new(area.x, y, area.width, footer_y.saturating_sub(y)),
        ScrollTarget::History,
    ));
    let columns = u16::from(!short);
    let free_rows = footer_y.saturating_sub(y + 1 + columns) as usize;
    let count = app.history.len();
    let row_step = if roomy && count > 0 && free_rows >= count * 2 - 1 {
        2
    } else {
        1
    };
    let available_rows = free_rows.div_ceil(row_step);
    let start = app.recent.saturating_sub(available_rows.saturating_sub(1));
    let count_label = if available_rows > 0 && available_rows < count {
        format!(
            "{}–{}/{}",
            (start + 1).min(count),
            (start + available_rows).min(count),
            count
        )
    } else {
        format!("{count} events")
    };
    let label = if narrow {
        format!("03 Recent · {count_label}")
    } else {
        format!("03 Recent launches · {count_label}")
    };
    if y < footer_y {
        section(
            f,
            at(area, y, 1),
            &label,
            "Ctrl+R",
            app.focus == Focus::History,
        );
    }
    y += 1;
    if !short && y < footer_y {
        history_row(
            f,
            at(area, y, 1),
            "",
            "TOOL",
            "DIRECTORY",
            "WHEN",
            theme.muted(),
            !narrow,
        );
        y += 1;
    }
    if app.history.is_empty() && y < footer_y {
        row(
            f,
            at(area, y, 1),
            "No recent launches. Choose a directory.",
            theme.muted(),
        );
    }
    for (i, event) in app
        .history
        .iter()
        .enumerate()
        .skip(start)
        .take(available_rows)
    {
        let active = i == app.recent && app.focus == Focus::History;
        let style = if active {
            theme.accent().bg(theme.raised)
        } else {
            theme.base()
        };
        let a = at(area, y, 1);
        hits.add(a, Action::SelectHistory(event.id));
        f.render_widget(Block::default().style(style), a);
        let marker = if active {
            "›".into()
        } else {
            format!("{:02}", i + 1)
        };
        let available = app.available(event.tool);
        let tool = format!(
            "{}{}",
            if available { "" } else { "!" },
            app.tool_label(event.tool)
        );
        history_row(
            f,
            a,
            &marker,
            &tool,
            &app.path_label(&event.path),
            if available { &event.age } else { "unavailable" },
            style,
            !narrow,
        );
        y += row_step as u16;
    }
    let foot = at(area, footer_y, 1);
    let global = if narrow {
        [
            ("F1", Action::Help),
            ("F5", Action::Reset),
            ("^Q", Action::Quit),
        ]
    } else {
        [
            ("F1 help", Action::Help),
            ("F5 reset", Action::Reset),
            ("Ctrl+Q quit", Action::Quit),
        ]
    };
    let global_width = global.iter().map(|(s, _)| width(s) as u16).sum::<u16>() + 6;
    let left = Rect::new(
        foot.x,
        foot.y,
        foot.width.saturating_sub(global_width + 1),
        1,
    );
    controls(
        f,
        hits,
        Rect::new(foot.right() - global_width, foot.y, global_width, 1),
        &global,
    );
    let hints = match (app.focus, narrow) {
        (Focus::Path, true) => "PATH Tab next · ↑↓",
        (Focus::Path, false) => "PATH Tab next section · ↑↓ select",
        (Focus::Tools, true) => "TOOLS Tab next · ←→",
        (Focus::Tools, false) => "TOOLS Tab next · ←→ choose · Enter",
        (Focus::History, true) => "RECENT Tab next · ↑↓",
        (Focus::History, false) => "RECENT Tab next · ↑↓ select · Enter replay",
    };
    row(f, left, hints, theme.accent().bg(theme.surface));
}

fn suggestions(f: &mut Canvas, app: &App, area: Rect, hits: &mut HitMap) {
    let theme = f.theme;
    if app.suggestions.is_empty() {
        row(
            f,
            at(area, area.y, 1),
            if area.width < 60 {
                &app.search_status
            } else if app.message.is_some() {
                "Edit the path or choose another directory."
            } else {
                "Enter launches · edit to search · ↓ tools"
            },
            theme.muted(),
        );
        return;
    }
    hits.wheels.push((area, ScrollTarget::Suggestions));
    let start = app
        .highlighted
        .unwrap_or(0)
        .saturating_sub(area.height.saturating_sub(1) as usize);
    for (r, (i, &index)) in app
        .suggestions
        .iter()
        .enumerate()
        .skip(start)
        .take(area.height as usize)
        .enumerate()
    {
        let d = &app.dirs[index];
        let a = at(area, area.y + r as u16, 1);
        hits.add(a, Action::AcceptSuggestion(index));
        let active = app.highlighted == Some(i);
        let style = if active {
            theme.accent().bg(theme.raised)
        } else {
            theme.base()
        };
        f.render_widget(Block::default().style(style), a);
        let label = format!(
            " {} {}/",
            if active { "›" } else { "·" },
            app.path_label(&d.path)
        );
        if area.width >= 70 {
            pair(f, a, &label, d.note, style);
        } else {
            row(f, a, &label, style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn history_row(
    f: &mut Canvas,
    a: Rect,
    marker: &str,
    tool: &str,
    path: &str,
    age: &str,
    style: Style,
    show_age: bool,
) {
    let theme = f.theme;
    row(f, Rect::new(a.x, a.y, 3, 1), marker, style.fg(theme.muted));
    let tool_width = 20;
    let path_offset = 3 + tool_width + 1;
    row(f, Rect::new(a.x + 3, a.y, tool_width, 1), tool, style);
    let age_width = if show_age { 12 } else { 0 };
    row(
        f,
        Rect::new(
            a.x + path_offset,
            a.y,
            a.width.saturating_sub(path_offset + age_width),
            1,
        ),
        path,
        style,
    );
    if show_age {
        let w = width(age) as u16;
        row(
            f,
            Rect::new(a.right() - w, a.y, w, 1),
            age,
            style.fg(theme.muted),
        );
    }
}

fn help(f: &mut Canvas, app: &App, area: Rect, hits: &mut HitMap) {
    let theme = f.theme;
    hits.wheels.push((
        Rect::new(area.x, area.y + 1, area.width, area.height - 2),
        ScrollTarget::Help,
    ));
    pair(
        f,
        at(area, area.y, 1),
        "Launchpad / help",
        if app.simulate_launch {
            "SUPPRESSED"
        } else {
            "REPLACE PANE"
        },
        theme.accent(),
    );
    let lines: Vec<_> = crate::help::lines(app).into_iter().map(Line::raw).collect();
    let content = Rect::new(area.x, area.y + 2, area.width, area.height - 3);
    let paragraph = Paragraph::new(lines)
        .style(theme.base())
        .wrap(Wrap { trim: false });
    // Use the same Ratatui cell/grapheme wrapper for measuring and painting.
    let max = paragraph
        .line_count(content.width)
        .saturating_sub(content.height as usize);
    app.help_scroll_max.set(max);
    f.render_widget(
        paragraph.scroll((app.help_scroll.min(max) as u16, 0)),
        content,
    );
    let foot = at(area, area.bottom() - 1, 1);
    row(f, foot, "↑↓ / wheel scroll · Home/End", theme.accent());
    controls(
        f,
        hits,
        Rect::new(foot.right() - 8, foot.y, 8, 1),
        &[("Esc back", Action::Escape)],
    );
}
