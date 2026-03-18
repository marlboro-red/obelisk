use super::*;
use super::agent_detail::render_vt100_screen_plain;

pub(super) fn render_split_pane(f: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;
    let pane_count = app.split_pane_count(area.width);

    if pane_count <= 1 {
        let block = primary_block("SPLIT VIEW — terminal too narrow for multi-pane", t);
        let p = Paragraph::new("Widen your terminal to at least 80 columns for 2-up, or 160 for 4-up")
            .block(block)
            .style(Style::default().fg(t.muted));
        f.render_widget(p, area);
        return;
    }

    let pane_rects = if pane_count == 2 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        vec![chunks[0], chunks[1]]
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);
        let bot = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);
        vec![top[0], top[1], bot[0], bot[1]]
    };

    for (slot, &pane_rect) in pane_rects.iter().enumerate() {
        let agent_id = app.split_pane_agents[slot];
        let agent = agent_id.and_then(|id| app.agents.iter().find(|a| a.id == id));
        let is_focused = slot == app.split_pane_focus;
        let is_pinned = agent.map(|a| a.pinned_to_split == Some(slot)).unwrap_or(false);

        match agent {
            Some(agent) => {
                let status_str = match agent.status {
                    AgentStatus::Starting => "INIT",
                    AgentStatus::Running => "▶",
                    AgentStatus::Completed => "✓",
                    AgentStatus::Failed => "✗",
                };

                let pin_indicator = if is_pinned { " [PIN]" } else { "" };
                let title = format!(
                    " AGENT-{:02} {} {} {} {}{}",
                    agent.unit_number,
                    status_str,
                    agent.task.id,
                    App::format_elapsed(agent.elapsed_secs),
                    agent.phase.short(),
                    pin_indicator,
                );

                let border_color = if is_focused { t.primary } else { t.muted };

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(if is_focused { t.bright } else { t.muted })
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(t.panel_bg));

                let inner = block.inner(pane_rect);
                f.render_widget(block, pane_rect);

                if let Some(pty_state) = app.pty_states.get(&agent.id) {
                    render_vt100_screen_plain(f, pty_state.parser.screen(), inner);
                } else {
                    let visible_height = inner.height as usize;
                    let total_lines = agent.output.len();
                    let max_scroll = total_lines.saturating_sub(visible_height);
                    let scroll = match app.split_pane_scroll[slot] {
                        None => max_scroll,
                        Some(pos) => pos.min(max_scroll),
                    };

                    let lines: Vec<Line> = agent
                        .output
                        .iter()
                        .skip(scroll)
                        .take(visible_height)
                        .map(|line: &String| {
                            Line::from(Span::styled(line.as_str(), Style::default().fg(t.accent)))
                        })
                        .collect();

                    let paragraph = Paragraph::new(lines);
                    f.render_widget(paragraph, inner);
                }
            }
            None => {
                let border_color = if is_focused { t.primary } else { t.muted };

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        format!(" PANE {} — empty ", slot + 1),
                        Style::default().fg(t.muted),
                    ))
                    .style(Style::default().bg(t.panel_bg));

                let p = Paragraph::new(Line::from(Span::styled(
                    "  No agent assigned",
                    Style::default().fg(t.muted),
                )))
                .block(block);
                f.render_widget(p, pane_rect);
            }
        }
    }
}
