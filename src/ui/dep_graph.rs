use super::*;
use super::helpers::{dep_status_color, dep_status_symbol};

pub(super) fn render_dep_graph(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let total = app.dep_graph_rows.len();
    let closed_count = app.dep_graph_rows.iter().filter(|r| r.node.status == "closed").count();
    let in_progress_count = app.dep_graph_rows.iter().filter(|r| r.node.status == "in_progress").count();
    let blocked_count = app.dep_graph_rows.iter().filter(|r| r.node.status == "blocked").count();

    let title = format!(
        "◆ DEPENDENCY GRAPH [{}]",
        total,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    if app.dep_graph_rows.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No dependency data loaded",
                Style::default().fg(t.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Data will load automatically...",
                Style::default().fg(t.muted),
            )),
        ])
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    // Layout: summary bar on top, tree list below
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Summary bar
            Constraint::Min(5),   // Tree list
        ])
        .split(block.inner(area));

    f.render_widget(block, area);

    // Summary bar
    let summary = Line::from(vec![
        Span::styled("  TOTAL: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", total),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    DONE: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", closed_count),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    IN PROGRESS: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", in_progress_count),
            Style::default().fg(t.info).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    BLOCKED: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", blocked_count),
            if blocked_count > 0 {
                Style::default().fg(t.danger).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            },
        ),
    ]);
    let summary_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(t.muted))
        .style(Style::default().bg(t.panel_bg));
    f.render_widget(
        Paragraph::new(summary).block(summary_block),
        v_chunks[0],
    );

    // Tree list
    let items: Vec<ListItem> = app
        .dep_graph_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let depth = row.node.depth;
            let indent = "  ".repeat(depth);

            // Tree connector
            let tree_prefix = if row.has_children {
                if row.collapsed { "▶ " } else { "▼ " }
            } else {
                "  "
            };

            // Selection indicator
            let sel_indicator = if Some(i) == app.dep_graph_list_state.selected() {
                Span::styled(
                    " ▸ ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            let status_color = dep_status_color(&row.node.status, t);
            let status_sym = dep_status_symbol(&row.node.status);

            // Priority badge
            let priority_str = row.node.priority
                .map(|p| format!("P{} ", p))
                .unwrap_or_default();

            // Type badge
            let type_str = row.node.issue_type
                .as_deref()
                .map(|t| format!("[{}] ", t))
                .unwrap_or_default();

            // Title (truncate to fit)
            let max_title = 50usize;
            let title = if row.node.title.len() > max_title {
                format!("{}...", &row.node.title[..max_title.saturating_sub(3)])
            } else {
                row.node.title.clone()
            };

            let row_bg = if row.node.status == "blocked" {
                Style::default().bg(Color::Rgb(35, 20, 20))
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                sel_indicator,
                Span::styled(
                    format!("{}{}", indent, tree_prefix),
                    Style::default().fg(t.muted),
                ),
                Span::styled(
                    format!("{} ", status_sym),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{}] ", row.node.id),
                    Style::default().fg(t.muted),
                ),
                Span::styled(
                    priority_str,
                    Style::default().fg(t.warn),
                ),
                Span::styled(
                    type_str,
                    Style::default().fg(t.muted),
                ),
                Span::styled(
                    title,
                    Style::default().fg(if row.node.status == "closed" { t.muted } else { t.bright }),
                ),
            ])).style(row_bg)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 35))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, v_chunks[1], &mut app.dep_graph_list_state.clone());

    // Scrollbar if needed
    if total > v_chunks[1].height as usize {
        let pos = app.dep_graph_list_state.selected().unwrap_or(0);
        let mut scrollbar_state = ScrollbarState::new(total).position(pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(t.muted)),
            v_chunks[1],
            &mut scrollbar_state,
        );
    }
}
