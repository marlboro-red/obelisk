use super::*;

pub(super) fn render_issue_creation_form(f: &mut Frame, area: Rect, app: &App) {
    use crate::types::ISSUE_TYPES;

    let t = &app.theme;
    let form = &app.issue_creation_form;

    let popup_width = 64u16.min(area.width.saturating_sub(4));
    let popup_height = 18u16.min(area.height.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.primary))
        .title(Span::styled(
            " CREATE ISSUE ",
            Style::default()
                .fg(t.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let focused = form.focused_field;
    let max_input_width = inner.width.saturating_sub(16) as usize;

    let field_label = |label: &str, idx: usize| -> Vec<Span<'_>> {
        let marker = if focused == idx { "> " } else { "  " };
        let style = if focused == idx {
            Style::default().fg(t.primary).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.muted)
        };
        vec![Span::styled(format!("{}{:12}", marker, label), style)]
    };

    let cursor = if app.frame_count % 10 < 5 { "█" } else { " " };

    // Title field
    let title_display = if form.title.len() > max_input_width {
        &form.title[form.title.len() - max_input_width..]
    } else {
        &form.title
    };
    let mut title_spans = field_label("Title:", 0);
    title_spans.push(Span::styled(
        title_display.to_string(),
        Style::default().fg(t.bright),
    ));
    if focused == 0 {
        title_spans.push(Span::styled(cursor, Style::default().fg(t.primary)));
    }

    // Description field
    let desc_display = if form.description.len() > max_input_width {
        &form.description[form.description.len() - max_input_width..]
    } else {
        &form.description
    };
    let mut desc_spans = field_label("Description:", 1);
    desc_spans.push(Span::styled(
        desc_display.to_string(),
        Style::default().fg(t.bright),
    ));
    if focused == 1 {
        desc_spans.push(Span::styled(cursor, Style::default().fg(t.primary)));
    }

    // Type field (cycle with Up/Down)
    let mut type_spans = field_label("Type:", 2);
    for (i, issue_type) in ISSUE_TYPES.iter().enumerate() {
        if i == form.issue_type_idx {
            type_spans.push(Span::styled(
                format!("[{}]", issue_type),
                Style::default()
                    .fg(t.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            type_spans.push(Span::styled(
                format!(" {} ", issue_type),
                Style::default().fg(t.muted),
            ));
        }
    }

    // Priority field (cycle with Up/Down)
    let mut priority_spans = field_label("Priority:", 3);
    for p in 1..=4 {
        let label = match p {
            1 => "P1",
            2 => "P2",
            3 => "P3",
            _ => "P4",
        };
        if p == form.priority {
            priority_spans.push(Span::styled(
                format!("[{}]", label),
                Style::default()
                    .fg(t.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            priority_spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(t.muted),
            ));
        }
    }

    let lines = vec![
        Line::from(""),
        Line::from(title_spans),
        Line::from(""),
        Line::from(desc_spans),
        Line::from(""),
        Line::from(type_spans),
        Line::from(""),
        Line::from(priority_spans),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Tab",
                Style::default()
                    .fg(t.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" next field  ", Style::default().fg(t.muted)),
            Span::styled(
                "Up/Down",
                Style::default()
                    .fg(t.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cycle option  ", Style::default().fg(t.muted)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Enter",
                Style::default()
                    .fg(t.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" create       ", Style::default().fg(t.muted)),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(t.danger)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancel", Style::default().fg(t.muted)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.panel_bg)),
        inner,
    );
}
