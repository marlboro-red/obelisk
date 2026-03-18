use super::*;

pub(super) fn render_history(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let (total_sessions, all_completed, all_failed, avg_duration, total_cost) = app.aggregate_stats();
    let all_time_total = all_completed + all_failed;
    let success_rate = if all_time_total > 0 {
        all_completed as f64 / all_time_total as f64 * 100.0
    } else {
        0.0
    };

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // Aggregate stats panel
            Constraint::Min(5),    // Session list
        ])
        .split(area);

    // ── Aggregate stats ──
    let stats_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " ALL-TIME STATISTICS ",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let stats_lines = vec![
        Line::from(vec![
            Span::styled("  SESSIONS        ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", total_sessions), Style::default().fg(t.bright).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  COMPLETED       ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", all_completed), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  FAILED          ", Style::default().fg(t.muted)),
            Span::styled(
                format!("{}", all_failed),
                if all_failed > 0 {
                    Style::default().fg(t.danger).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.muted)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("  SUCCESS RATE    ", Style::default().fg(t.muted)),
            Span::styled(
                format!("{:.1}%", success_rate),
                if success_rate >= 80.0 {
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
                } else if success_rate >= 50.0 {
                    Style::default().fg(t.warn).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.danger).add_modifier(Modifier::BOLD)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("  AVG DURATION    ", Style::default().fg(t.muted)),
            Span::styled(
                App::format_elapsed(avg_duration as u64).to_string(),
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  TOTAL COST      ", Style::default().fg(t.muted)),
            Span::styled(
                crate::cost::format_cost(total_cost),
                if total_cost > 0.0 {
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.muted)
                },
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(stats_lines).block(stats_block), v_chunks[0]);

    // ── Session list ──
    let sessions_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            format!(" SESSION LOG [{}] ", app.history_sessions.len()),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    if app.history_sessions.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::styled("  No sessions recorded yet — ", Style::default().fg(t.muted)),
            Span::styled("run some agents and quit to record your first session", Style::default().fg(t.warn)),
        ]))
        .block(sessions_block);
        f.render_widget(empty, v_chunks[1]);
        return;
    }

    let inner = sessions_block.inner(v_chunks[1]);
    let visible = inner.height as usize;

    // Sessions are stored oldest-first; display newest-first
    let sessions_newest_first: Vec<_> = app.history_sessions.iter().rev().collect();

    let items: Vec<ListItem> = sessions_newest_first
        .iter()
        .skip(app.history_scroll)
        .take(visible)
        .map(|session| {
            let total = session.total_completed + session.total_failed;
            let rate = if total > 0 {
                session.total_completed as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let rate_color = if rate >= 80.0 { t.accent } else if rate >= 50.0 { t.warn } else { t.danger };
            let short_id = if session.session_id.len() > 16 {
                &session.session_id[..16]
            } else {
                &session.session_id
            };
            let cost_str = if session.total_cost_usd > 0.0 {
                crate::cost::format_cost(session.total_cost_usd)
            } else {
                "--".to_string()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:16} ", short_id), Style::default().fg(t.muted)),
                Span::styled(format!("started: {:<22} ", &session.started_at[..session.started_at.len().min(19)]), Style::default().fg(t.muted)),
                Span::styled(format!("done:{:>4} ", session.total_completed), Style::default().fg(t.accent)),
                Span::styled(format!("fail:{:>3} ", session.total_failed), Style::default().fg(if session.total_failed > 0 { t.danger } else { t.muted })),
                Span::styled(format!("{:>5.1}%", rate), Style::default().fg(rate_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {:>8}", cost_str), Style::default().fg(if session.total_cost_usd > 0.0 { t.accent } else { t.muted })),
                Span::styled(format!("  {:>3} agents", session.agents.len()), Style::default().fg(t.muted)),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(sessions_block), v_chunks[1]);
}
