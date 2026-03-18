use super::*;
use super::helpers::truncate_str;

pub(super) fn render_worktree_overview(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    use crate::types::WorktreeStatus;

    let total = app.worktree_entries.len();
    let orphaned_count = app.worktree_entries.iter().filter(|e| e.status == WorktreeStatus::Orphaned).count();
    let active_count = app.worktree_entries.iter().filter(|e| e.status == WorktreeStatus::Active).count();

    let title = format!(
        "◆ WORKTREE OVERVIEW [{}] [sort: {}]",
        total,
        app.worktree_sort_mode.label(),
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

    if app.worktree_entries.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No agent worktrees found",
                Style::default().fg(t.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Worktrees will appear here when agents are spawned",
                Style::default().fg(t.muted),
            )),
        ])
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    // Layout: summary bar on top, list below
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Summary bar
            Constraint::Min(5),   // Worktree list
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
        Span::styled("    ACTIVE: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", active_count),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    ORPHANED: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", orphaned_count),
            if orphaned_count > 0 {
                Style::default().fg(t.danger).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            },
        ),
        if orphaned_count > 0 {
            Span::styled(
                "  — press 'c' on dashboard to clean up",
                Style::default().fg(t.warn),
            )
        } else {
            Span::styled("", Style::default())
        },
    ]);
    let summary_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(t.muted))
        .style(Style::default().bg(t.panel_bg));
    f.render_widget(
        Paragraph::new(summary).block(summary_block),
        v_chunks[0],
    );

    // Worktree list
    let items: Vec<ListItem> = app
        .worktree_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let (status_sym, status_color) = match entry.status {
                WorktreeStatus::Active => ("▶ ACTIVE  ", t.accent),
                WorktreeStatus::Idle => ("● IDLE    ", t.warn),
                WorktreeStatus::Orphaned => ("✗ ORPHAN  ", t.danger),
            };

            let sel_indicator = if Some(i) == app.worktree_list_state.selected() {
                Span::styled(
                    " ▸ ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            // Worktree directory name (short)
            let dir_name = std::path::Path::new(&entry.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.path);

            // Age
            let age_str = entry.created_at.map(|t| {
                let dur = chrono::Local::now().signed_duration_since(t);
                if dur.num_days() > 0 {
                    format!("{}d ago", dur.num_days())
                } else if dur.num_hours() > 0 {
                    format!("{}h ago", dur.num_hours())
                } else {
                    format!("{}m ago", dur.num_minutes().max(1))
                }
            }).unwrap_or_else(|| "? ago".to_string());

            // Issue ID
            let issue_span = if let Some(ref id) = entry.issue_id {
                Span::styled(format!("[{}] ", id), Style::default().fg(t.muted))
            } else {
                Span::styled("[???] ", Style::default().fg(t.muted))
            };

            // Branch
            let branch_span = Span::styled(
                format!("({})", entry.branch),
                Style::default().fg(t.muted),
            );

            // Build line with color coding — orphaned entries use danger styling
            let row_bg = if entry.status == WorktreeStatus::Orphaned {
                Style::default().bg(Color::Rgb(35, 20, 20))
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                sel_indicator,
                Span::styled(
                    status_sym,
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                issue_span,
                Span::styled(
                    format!("{:<30} ", truncate_str(dir_name, 30)),
                    Style::default().fg(t.bright),
                ),
                branch_span,
                Span::styled(format!("  {}", age_str), Style::default().fg(t.muted)),
            ])).style(row_bg)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 35))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, v_chunks[1], &mut app.worktree_list_state.clone());

    // Scrollbar if needed
    if total > v_chunks[1].height as usize {
        let pos = app.worktree_list_state.selected().unwrap_or(0);
        let mut scrollbar_state = ScrollbarState::new(total).position(pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(t.muted)),
            v_chunks[1],
            &mut scrollbar_state,
        );
    }
}
