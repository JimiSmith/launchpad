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
            Paragraph::new(Line::styled(clip(text, area.width as usize), style))
                .style(style.remove_modifier(Modifier::UNDERLINED)),
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
                theme.muted(),
            );
            x += 3;
        }
        let w = width(label) as u16;
        if x + w > area.right() || area.height == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, w, 1);
        row(f, rect, label, theme.muted());
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
    let width = viewport.width.min(96);
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

// Use explicit foreground colours: SGR DIM is not reliably preserved by hosts.
fn dim_section(f: &mut Canvas, area: Rect, active: bool) {
    if !active {
        let area = area.intersection(f.area());
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let cell = &mut f.buffer[(x, y)];
                cell.set_fg(f.theme.inactive(cell.fg));
            }
        }
    }
}

fn dashboard(f: &mut Canvas, app: &App, area: Rect, hits: &mut HitMap) {
    let theme = f.theme;
    let short = area.height < 18;
    let roomy = area.height >= 30;
    let footer_y = area.bottom() - 1 - u16::from(roomy);
    let mut y = area.y + if roomy { 2 } else { u16::from(!short) };
    pair(
        f,
        at(area, y, 1),
        "›_ Launchpad",
        if app.simulate_launch { "Preview" } else { "" },
        theme.muted(),
    );
    y += if roomy {
        3
    } else if short {
        1
    } else {
        2
    };

    let directory_y = y;
    section(f, at(area, y, 1), "Directory", "", app.focus == Focus::Path);
    y += 1;
    let input = at(area, y, if short { 1 } else { 2 });
    let block = Block::default()
        .borders(if short {
            Borders::NONE
        } else {
            Borders::BOTTOM
        })
        .border_style(if app.focus == Focus::Path {
            theme.accent()
        } else {
            theme.base().fg(theme.border)
        })
        .style(theme.base());
    f.render_widget(block, input);
    row(f, Rect::new(input.x, input.y, 2, 1), "›", theme.accent());
    let text_area = Rect::new(input.x + 2, input.y, input.width.saturating_sub(2), 1);
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
    row(f, text_area, &visible, theme.base());
    if app.focus == Focus::Path && text_area.width > 0 {
        f.set_cursor_position((text_area.x + caret as u16, text_area.y));
    }
    y += input.height;
    let suggestion_rows = if app.suggestions.is_empty() {
        u16::from(app.empty_search())
    } else {
        app.suggestions.len().min(if short {
            1
        } else if roomy {
            4
        } else {
            3
        }) as u16
    };
    suggestions(f, app, at(area, y, suggestion_rows), hits);
    y += suggestion_rows;
    dim_section(
        f,
        Rect::new(area.x, directory_y, area.width, y - directory_y),
        app.focus == Focus::Path,
    );
    y += if short {
        0
    } else if roomy {
        2
    } else {
        1
    };

    let tools_y = y;
    section(f, at(area, y, 1), "Tool", "", app.focus == Focus::Tools);
    y += 1;
    // Reserve space beside the tools for Launch, even when labels wrap.
    let tool_width = area.width.saturating_sub(12);
    let mut tools = app.visible_tools();
    if !app.available(app.tool) {
        tools.push(app.tool);
    }
    let selected = tools.iter().position(|&t| t == app.tool).unwrap_or(0);
    let mut positions = Vec::new();
    let (mut column, mut tool_row) = (0u16, 0u16);
    for &tool in &tools {
        let label = if app.available(tool) {
            app.tool_label(tool)
        } else {
            format!("! {}", app.tool_label(tool))
        };
        let label = clip(&label, tool_width as usize);
        let w = width(&label) as u16;
        if column > 0 && column + w > tool_width {
            column = 0;
            tool_row += 1;
        }
        positions.push((tool_row, column, w, label));
        column += w + 3;
    }
    let max_rows = if area.height < 12 || (!short && area.height < 24) {
        1
    } else {
        2
    };
    let start = positions[selected].0.saturating_sub(max_rows - 1);
    let visible_rows = (tool_row + 1).min(max_rows);
    let mut end_x = area.x;
    for (&tool, (r, col, w, label)) in tools.iter().zip(&positions) {
        if *r < start || *r >= start + visible_rows {
            continue;
        }
        let style = if app.tool == tool {
            theme.accent().add_modifier(Modifier::UNDERLINED)
        } else {
            theme.muted()
        };
        let rect = Rect::new(area.x + col, y + r - start, *w, 1);
        row(f, rect, label, style);
        hits.add(rect, Action::SelectTool(tool));
        end_x = end_x.max(rect.right());
    }
    if tool_row + 1 > visible_rows {
        let hint = format!("‹ {}/{} ›", selected + 1, tools.len());
        let w = width(&hint) as u16;
        let heading = Rect::new(area.right() - w, tools_y, w, 1);
        row(f, heading, &hint, theme.muted());
        hits.add(
            Rect::new(heading.x, heading.y, 1, 1),
            Action::SelectTool(tools[(selected + tools.len() - 1) % tools.len()]),
        );
        hits.add(
            Rect::new(heading.right() - 1, heading.y, 1, 1),
            Action::SelectTool(tools[(selected + 1) % tools.len()]),
        );
    }
    let launch = Rect::new(end_x + 4, y, 8, 1);
    row(f, launch, "Launch ↵", theme.accent());
    hits.add(launch, Action::LaunchForm);
    y += visible_rows;
    dim_section(
        f,
        Rect::new(area.x, tools_y, area.width, y - tools_y),
        app.focus == Focus::Tools,
    );
    y += if short {
        1
    } else if roomy {
        3
    } else {
        2
    };

    // Keep errors visible, with room for a recent row and the footer at small sizes.
    let config_error = app.config_errors().next().map(|e| {
        format!(
            "Config error: {e} (F1: all {} errors)",
            app.config_errors().count()
        )
    });
    if let Some(message) = app.message.as_ref().or(config_error.as_ref()) {
        let max_height = footer_y.saturating_sub(y + 2).clamp(1, 3);
        let paragraph = Paragraph::new(message.as_str())
            .style(theme.base().fg(theme.error))
            .wrap(Wrap { trim: false });
        let h = (paragraph.line_count(area.width) as u16).clamp(1, max_height);
        // On the smallest screen use the section gap for the status.
        if short {
            y = y.saturating_sub(1);
        }
        f.render_widget(paragraph, at(area, y, h));
        y += h;
    }
    let history_y = y;
    let free_rows = footer_y.saturating_sub(y + 1 + u16::from(!short)) as usize;
    let row_step = if roomy { 2 } else { 1 };
    let available_rows = free_rows.div_ceil(row_step);
    let count = app.history.len();
    let start = app.recent.saturating_sub(available_rows.saturating_sub(1));
    let range = if available_rows > 0 && available_rows < count {
        format!(
            "{}–{} / {}",
            (start + 1).min(count),
            (start + available_rows).min(count),
            count
        )
    } else {
        String::new()
    };
    if y < footer_y {
        section(
            f,
            at(area, y, 1),
            "Recent",
            &range,
            app.focus == Focus::History,
        );
    }
    y += 1;
    hits.wheels.push((
        Rect::new(
            area.x,
            history_y,
            area.width,
            footer_y.saturating_sub(history_y),
        ),
        ScrollTarget::History,
    ));
    if app.history.is_empty() && y < footer_y {
        row(f, at(area, y, 1), "No recent launches", theme.muted());
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
            theme.accent().add_modifier(Modifier::UNDERLINED)
        } else {
            theme.base()
        };
        let a = at(area, y, 1);
        hits.add(a, Action::SelectHistory(event.id));
        let tool = if app.available(event.tool) {
            app.tool_label(event.tool)
        } else {
            format!("! {}", app.tool_label(event.tool))
        };
        let tool = clip(&tool, (a.width / 2) as usize);
        pair(f, a, &app.path_label(&event.path), &tool, style);
        y += row_step as u16;
    }
    dim_section(
        f,
        Rect::new(
            area.x,
            history_y,
            area.width,
            footer_y.saturating_sub(history_y),
        ),
        app.focus == Focus::History,
    );

    let foot = at(area, footer_y, 1);
    let hints = match app.focus {
        Focus::Path if area.width < 60 && app.highlighted.is_some() => "Tab next · ↵ choose",
        Focus::Tools if area.width < 60 => "Tab next · ←→ · ↵ launch",
        Focus::History if area.width < 60 => "Tab next · ↑↓ · ↵ launch",
        Focus::Path if app.highlighted.is_some() => "Tab next · ↵ choose directory",
        Focus::Path => "Tab next · ↵ launch",
        Focus::Tools => "Tab next · ←→ tool · ↵ launch",
        Focus::History => "Tab next · ↑↓ recent · ↵ launch",
    };
    row(
        f,
        Rect::new(foot.x, foot.y, foot.width - 9, 1),
        hints,
        theme.muted(),
    );
    controls(
        f,
        hits,
        Rect::new(foot.right() - 7, foot.y, 7, 1),
        &[("F1 help", Action::Help)],
    );
}

fn suggestions(f: &mut Canvas, app: &App, area: Rect, hits: &mut HitMap) {
    let theme = f.theme;
    if area.is_empty() {
        return;
    }
    if app.empty_search() {
        row(f, at(area, area.y, 1), "No suggestions", theme.muted());
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
        let active = app.highlighted == Some(i) && app.focus == Focus::Path;
        let style = if active {
            theme
                .accent()
                .bg(theme.raised)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            theme.base()
        };
        f.render_widget(
            Block::default().style(style.remove_modifier(Modifier::UNDERLINED)),
            a,
        );
        let label = format!("{}/", app.path_label(&d.path));
        row(f, a, &label, style);
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
