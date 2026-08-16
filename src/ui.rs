use crate::{
    app::{App, Focus, MessageLevel},
    entry::EntryKind,
    input::TextInput,
    plan::{PreviewRow, RowState, SelectionFilter},
};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

const PREVIEW_HEADERS: [&str; 3] = ["Type", "Before", "After"];

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let [input_area, status_area, preview_area, footer_area] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    render_inputs(frame, input_area, app);
    render_status(frame, status_area, app);
    render_preview(frame, preview_area, app);
    render_footer(frame, footer_area, app);

    if app.showing_blockers {
        render_blocking_details(frame, app);
    } else if app.confirming {
        render_confirmation(frame, app);
    };
}

fn render_inputs(frame: &mut Frame, area: Rect, app: &App) {
    let pattern = editable_line(
        "Pattern",
        &app.pattern,
        app.focus == Focus::Pattern,
    );
    let replacement = editable_line(
        "Replacement",
        &app.replacement,
        app.focus == Focus::Replacement,
    );
    let help = Line::from(vec![
        Span::styled("Regex syntax", Style::default().fg(Color::DarkGray)),
        Span::raw(" · captures in replacements use "),
        Span::styled("$1", Style::default().fg(Color::Cyan)),
        Span::raw(" or "),
        Span::styled("${name}", Style::default().fg(Color::Cyan)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename expression ")
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(vec![pattern, replacement, help])
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn editable_line(label: &str, input: &TextInput, focused: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<12}"),
        Style::default().add_modifier(Modifier::BOLD),
    )];

    if focused {
        let value = input.value();
        let cursor = input.cursor();
        let left = value[..cursor].to_owned();
        let mut remainder = value[cursor..].chars();
        let cursor_character = remainder
            .next()
            .map_or_else(|| " ".to_owned(), |character| character.to_string());
        let right = remainder.as_str().to_owned();
        spans.push(Span::raw(left));
        spans.push(Span::styled(
            cursor_character,
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        spans.push(Span::raw(right));
    } else {
        spans.push(Span::raw(input.value().to_owned()));
    }

    Line::from(spans)
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = status_content(app);
    frame.render_widget(
        Paragraph::new(text)
            .style(style)
            .block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn status_content(app: &App) -> (String, Style) {
    if let Some(message) = &app.message {
        let color = match message.level {
            MessageLevel::Info => Color::Cyan,
            MessageLevel::Success => Color::Green,
            MessageLevel::Error => Color::Red,
        };
        return (message.text.clone(), Style::default().fg(color));
    }
    if let Some(error) = &app.plan.regex_error {
        return (
            format!("Invalid regex: {error}"),
            Style::default().fg(Color::Red),
        );
    }
    if app.plan.pattern_is_empty {
        return (
            "Enter a regex pattern to build a rename preview.".to_owned(),
            Style::default().fg(Color::DarkGray),
        );
    }

    (
        format!(
            "{} ready · {} unchanged · {} conflicts · {} permission warnings",
            app.plan.ready_count(),
            app.plan.unchanged_count(),
            app.plan.conflict_count(),
            app.plan.warning_count()
        ),
        Style::default(),
    )
}

fn render_preview(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app
        .plan
        .rows
        .iter()
        .skip(app.scroll)
        .map(preview_table_row);
    let header = Row::new(PREVIEW_HEADERS)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);
    let title = if app.plan.rows.is_empty() {
        " Preview · no entries ".to_owned()
    } else {
        format!(
            " Preview · starting at row {} of {} ",
            app.scroll.saturating_add(1),
            app.plan.rows.len()
        )
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Percentage(47),
            Constraint::Percentage(47),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Blue)),
    );
    frame.render_widget(table, area);
}

fn preview_table_row(row: &PreviewRow) -> Row<'static> {
    let kind = match row.kind {
        EntryKind::File => "file",
        EntryKind::Directory => "dir",
    };
    Row::new(vec![
        Cell::from(kind),
        Cell::from(before_line(row)),
        Cell::from(row.after.clone()),
    ])
    .style(preview_row_style(row))
}

fn before_line(row: &PreviewRow) -> Line<'static> {
    if row.has_conflict() || row.match_ranges.is_empty() {
        return Line::from(row.before.clone());
    }

    let mut spans = Vec::with_capacity(
        row.match_ranges
            .len()
            .saturating_mul(2)
            .saturating_add(1),
    );
    let mut cursor = 0;

    for range in &row.match_ranges {
        if range.start < cursor || range.start > range.end {
            return Line::from(row.before.clone());
        }
        let Some(prefix) = row.before.get(cursor..range.start) else {
            return Line::from(row.before.clone());
        };
        let Some(highlight) = row.before.get(range.start..range.end) else {
            return Line::from(row.before.clone());
        };

        if !prefix.is_empty() {
            spans.push(Span::raw(prefix.to_owned()));
        }
        if !highlight.is_empty() {
            spans.push(Span::styled(
                highlight.to_owned(),
                Style::default().fg(Color::Green),
            ));
        }
        cursor = range.end;
    }

    let Some(suffix) = row.before.get(cursor..) else {
        return Line::from(row.before.clone());
    };
    if !suffix.is_empty() {
        spans.push(Span::raw(suffix.to_owned()));
    }

    Line::from(spans)
}

fn preview_row_style(row: &PreviewRow) -> Style {
    if row.has_conflict() {
        return Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD);
    }

    if row.has_warning() {
        return Style::default().fg(Color::Yellow);
    }

    match row.state {
        RowState::NotSelected | RowState::Waiting | RowState::Unchanged => {
            Style::default().fg(Color::DarkGray)
        }
        RowState::Ready => Style::default(),
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let shortcuts = Line::from(vec![
        filter_span("F1 files", app.filter == SelectionFilter::Files),
        Span::raw(" · "),
        filter_span("F2 folders", app.filter == SelectionFilter::Directories),
        Span::raw(" · "),
        filter_span("F3 both", app.filter == SelectionFilter::Both),
        Span::raw(
            " · Tab field · Ctrl-A/E start/end · ↑/↓ scroll · Ctrl-R review · Ctrl-Q quit",
        ),
    ]);
    let caveat = Line::styled(
        "Permission warnings use Unix owner/group/mode/sticky-bit heuristics; ACLs and read-only mounts may differ.",
        Style::default().fg(Color::DarkGray),
    );
    frame.render_widget(Paragraph::new(vec![shortcuts, caveat]), area);
}

fn filter_span(label: &str, selected: bool) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Span::styled(label.to_owned(), style)
}

fn render_confirmation(frame: &mut Frame, app: &App) {
    let warning_details = app.plan.warning_details();
    let popup_height = u16::try_from(warning_details.len())
        .unwrap_or(u16::MAX)
        .saturating_add(9)
        .min(frame.area().height.saturating_sub(2))
        .max(8);
    let popup = centered_area(frame.area(), 82, popup_height);
    frame.render_widget(Clear, popup);

    let warnings = app.plan.warning_count();
    let mut lines = vec![
        Line::styled(
            format!(
                "Rename {} {}?",
                app.plan.actions.len(),
                plural_entry(app.plan.actions.len())
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
    ];
    if warning_details.is_empty() {
        lines.push(Line::from("No permission warnings were detected."));
    } else {
        lines.push(Line::styled(
            format!(
                "Permission warnings affect {warnings} {}:",
                plural_entry(warnings)
            ),
            Style::default().fg(Color::Yellow),
        ));
        lines.extend(warning_details.into_iter().map(|detail| {
            Line::styled(format!("- {detail}"), Style::default().fg(Color::Yellow))
        }));
    }
    lines.extend([
        Line::from(""),
        Line::from("Destinations will be rechecked before each direct rename."),
        Line::from(""),
        Line::styled(
            "Press y to rename, or n / Esc to cancel.",
            Style::default().fg(Color::Cyan),
        ),
    ]);
    let text = Text::from(lines);
    let border_color = if warnings == 0 {
        Color::Cyan
    } else {
        Color::Yellow
    };
    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm rename ")
                .border_style(Style::default().fg(border_color)),
        );
    frame.render_widget(paragraph, popup);
}

fn render_blocking_details(frame: &mut Frame, app: &App) {
    let details = app.plan.blocking_details();
    let popup_height = u16::try_from(details.len())
        .unwrap_or(u16::MAX)
        .saturating_add(7)
        .min(frame.area().height.saturating_sub(2))
        .max(8);
    let popup = centered_area(frame.area(), 84, popup_height);
    frame.render_widget(Clear, popup);

    let mut lines = vec![
        Line::styled(
            "The rename plan cannot be submitted.",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
    ];
    lines.extend(details.into_iter().map(|detail| {
        Line::styled(format!("- {detail}"), Style::default().fg(Color::Red))
    }));
    lines.extend([
        Line::from(""),
        Line::styled(
            "Press Enter or Esc to return to editing.",
            Style::default().fg(Color::Cyan),
        ),
    ]);

    let paragraph = Paragraph::new(Text::from(lines))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Cannot rename ")
                .border_style(Style::default().fg(Color::Red)),
        );
    frame.render_widget(paragraph, popup);
}

fn centered_area(area: Rect, width_percent: u16, height: u16) -> Rect {
    let width = (area.width.saturating_mul(width_percent) / 100)
        .max(24)
        .min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn plural_entry(count: usize) -> &'static str {
    if count == 1 {
        "entry"
    } else {
        "entries"
    }
}

#[cfg(test)]
mod tests {
    use super::{before_line, preview_row_style, PREVIEW_HEADERS};
    use crate::{
        entry::EntryKind,
        plan::{Issue, IssueLevel, PreviewRow, RowState},
    };
    use ratatui::{
        style::{Color, Modifier, Style},
        text::{Line, Span},
    };

    #[test]
    fn preview_has_no_status_column() {
        assert_eq!(PREVIEW_HEADERS, ["Type", "Before", "After"]);
    }

    #[test]
    fn before_highlights_only_the_matching_span() {
        let row = PreviewRow {
            kind: EntryKind::File,
            before: "prefix-match-suffix".to_owned(),
            after: "prefix-renamed-suffix".to_owned(),
            match_ranges: vec![7..12],
            state: RowState::Ready,
            issues: Vec::new(),
        };

        let line = before_line(&row);

        assert_eq!(preview_row_style(&row), Style::default());
        assert_eq!(
            line,
            Line::from(vec![
                Span::raw("prefix-"),
                Span::styled("match", Style::default().fg(Color::Green)),
                Span::raw("-suffix"),
            ]),
        );
    }

    #[test]
    fn conflicting_rows_remain_red() {
        let row = PreviewRow {
            kind: EntryKind::File,
            before: "before".to_owned(),
            after: "after".to_owned(),
            match_ranges: vec![0..6],
            state: RowState::Ready,
            issues: vec![Issue {
                level: IssueLevel::Conflict,
                message: "conflict".to_owned(),
            }],
        };

        let line = before_line(&row);
        let style = preview_row_style(&row);

        assert_eq!(line, Line::from("before"));
        assert_eq!(
            style,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        );
    }
}
