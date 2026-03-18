use super::*;
use super::helpers::*;

pub(super) fn render_help_overlay(f: &mut Frame, area: Rect, t: &Theme) {
    // Center a popup of fixed size
    let popup_width = 64u16.min(area.width.saturating_sub(4));
    let popup_height = 34u16.min(area.height.saturating_sub(4));
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
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " KEYBOARD SHORTCUTS  [? / Esc to close] ",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Build lines for all sections
    let mut lines: Vec<Line> = Vec::new();

    let section_header = |title: &'static str| -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("  ── {} ──", title),
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            ),
        ])
    };

    let key_line = |key: &'static str, desc: &'static str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {:12}", key), Style::default().fg(t.primary).add_modifier(Modifier::BOLD)),
            Span::styled(desc, Style::default().fg(t.bright)),
        ])
    };

    // ── Dashboard ──
    lines.push(section_header("DASHBOARD"));
    lines.push(key_line("s", "Spawn agent on selected task"));
    lines.push(key_line("p", "Trigger manual poll / scan"));
    lines.push(key_line("r", "Cycle runtime (Claude/Codex/Copilot)"));
    lines.push(key_line("m", "Cycle model for current runtime"));
    lines.push(key_line("a", "Toggle auto-spawn mode"));
    lines.push(key_line("n", "Toggle desktop notifications on/off"));
    lines.push(key_line("C", "Create a new beads issue"));
    lines.push(key_line("c", "Scan and clean up orphaned worktrees"));
    lines.push(key_line("f", "Cycle sort mode (priority/type/age/name)"));
    lines.push(key_line("F", "Cycle type filter (bug/feature/task/chore/epic)"));
    lines.push(key_line("/", "Jump to issue by ID"));
    lines.push(key_line("Tab", "Cycle focus: Ready → Blocked → Agents"));
    lines.push(key_line("↑↓ / j/k", "Navigate list  (detail panel updates)"));
    lines.push(key_line("Enter", "Open Agent Detail for selected"));
    lines.push(key_line("f", "Cycle agent status filter: All→Running→Failed→Done→Init (focus: Agents)"));
    lines.push(key_line("y", "Copy issue ID or agent info to clipboard"));
    lines.push(key_line("x", "Dismiss selected finished agent (focus: Agents)"));
    lines.push(key_line("X", "Dismiss ALL finished agents (focus: Agents)"));
    lines.push(key_line("+/-", "Increase/decrease max concurrent slots"));
    lines.push(key_line("1-7", "Switch view"));
    lines.push(key_line("q", "Quit"));
    lines.push(Line::from(""));

    // ── Agent Detail — Observe ──
    lines.push(section_header("AGENT DETAIL  (Observe mode)"));
    lines.push(key_line("i", "Attach interactive PTY session"));
    lines.push(key_line("d", "Toggle live git diff panel"));
    lines.push(key_line("J/K", "Scroll diff panel (when visible)"));
    lines.push(key_line("r", "Retry failed agent (spawn fresh PTY)"));
    lines.push(key_line("↑↓", "Scroll output one line"));
    lines.push(key_line("PgUp/PgDn", "Scroll output by page"));
    lines.push(key_line("Home/End", "Jump to top / re-engage auto-follow"));
    lines.push(key_line("←/→", "Previous / next agent"));
    lines.push(key_line("/", "Open search bar (searches visible PTY output)"));
    lines.push(key_line("n / N", "Next / previous search match"));
    lines.push(key_line("Esc (search)", "Close search bar"));
    lines.push(key_line("e", "Export agent log to file"));
    lines.push(key_line("y", "Copy worktree path to clipboard"));
    lines.push(key_line("D", "Mark agent as completed + SIGTERM + clean up worktree"));
    lines.push(key_line("k", "Kill (SIGTERM) current agent + clean up worktree"));
    lines.push(key_line("Esc / q", "Return to Dashboard"));
    lines.push(Line::from(""));

    // ── Agent Detail — Interactive ──
    lines.push(section_header("AGENT DETAIL  (Interactive mode)"));
    lines.push(key_line("F2", "Detach from PTY (return to Observe)"));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("All other keys are forwarded to the agent's PTY.", Style::default().fg(t.muted)),
    ]));
    lines.push(Line::from(""));

    // ── Event Log ──
    lines.push(section_header("EVENT LOG"));
    lines.push(key_line("↑↓", "Scroll log"));
    lines.push(key_line("f", "Cycle category filter"));
    lines.push(key_line("1-7", "Switch view"));
    lines.push(key_line("q", "Quit"));
    lines.push(Line::from(""));

    // ── History ──
    lines.push(section_header("HISTORY"));
    lines.push(key_line("↑↓", "Scroll session list"));
    lines.push(key_line("PgUp/PgDn", "Scroll by 10 sessions"));
    lines.push(key_line("1-7", "Switch view"));
    lines.push(key_line("q", "Quit"));
    lines.push(Line::from(""));

    // ── Split Pane ──
    lines.push(section_header("SPLIT PANE  (Multi-agent monitor)"));
    lines.push(key_line("Tab", "Cycle focus between panes"));
    lines.push(key_line("↑↓", "Scroll focused pane output"));
    lines.push(key_line("Enter", "Open Agent Detail for focused pane"));
    lines.push(key_line("g", "Pin/unpin agent to focused pane"));
    lines.push(key_line("Esc / q", "Return to Dashboard"));
    lines.push(Line::from(""));
    // ── Dependency Graph ──
    lines.push(section_header("DEPENDENCY GRAPH"));
    lines.push(key_line("↑↓ / j/k", "Navigate dependency list"));
    lines.push(key_line("Enter", "Expand/collapse subtree"));
    lines.push(key_line("Esc / q", "Return to Dashboard"));
    lines.push(Line::from(""));
    // ── Worktree Overview ──
    lines.push(section_header("WORKTREE OVERVIEW"));
    lines.push(key_line("↑↓ / j/k", "Navigate worktree list"));
    lines.push(key_line("f", "Cycle sort mode (age/status)"));
    lines.push(key_line("w", "Open worktree overview (from Dashboard)"));
    lines.push(key_line("Esc / q", "Return to Dashboard"));
    lines.push(Line::from(""));
    // ── Global ──
    lines.push(section_header("GLOBAL"));
    lines.push(key_line("?", "Toggle this help overlay"));
    lines.push(key_line("Ctrl+C", "Force quit"));

    let visible = inner.height as usize;
    let display: Vec<Line> = lines.into_iter().take(visible).collect();

    f.render_widget(Paragraph::new(display).style(Style::default().bg(t.panel_bg)), inner);
}

pub(super) fn render_complete_confirm_dialog(f: &mut Frame, area: Rect, app: &App, agent_id: usize) {
    let t = &app.theme;
    let (agent_label, issue_id) = app
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .map(|a| (format!("AGENT-{:02}", a.unit_number), a.task.id.clone()))
        .unwrap_or_else(|| (format!("AGENT-{}", agent_id), String::from("?")));

    let popup_width = 58u16.min(area.width.saturating_sub(4));
    let popup_height = 7u16.min(area.height.saturating_sub(4));
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
        .border_style(Style::default().fg(t.accent))
        .title(Span::styled(
            " MARK COMPLETE ",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  Mark {} ({}) as completed? ", agent_label, issue_id),
                Style::default().fg(t.bright),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  This will set status to Completed and terminate the process.",
                Style::default().fg(t.muted),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[y/Enter] ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled("Confirm   ", Style::default().fg(t.bright)),
            Span::styled("[n/Esc] ", Style::default().fg(t.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(t.bright)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.panel_bg)),
        inner,
    );
}

pub(super) fn render_kill_confirm_dialog(f: &mut Frame, area: Rect, app: &App, agent_id: usize) {
    let t = &app.theme;
    let (agent_label, issue_id) = app
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .map(|a| (format!("AGENT-{:02}", a.unit_number), a.task.id.clone()))
        .unwrap_or_else(|| (format!("AGENT-{}", agent_id), String::from("?")));

    let popup_width = 58u16.min(area.width.saturating_sub(4));
    let popup_height = 7u16.min(area.height.saturating_sub(4));
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
        .border_style(Style::default().fg(t.danger))
        .title(Span::styled(
            " CONFIRM KILL ",
            Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  Kill {} ({})? ", agent_label, issue_id),
                Style::default().fg(t.bright),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  This will terminate the process and clean up the worktree.",
                Style::default().fg(t.muted),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y/Enter] ", Style::default().fg(t.danger).add_modifier(Modifier::BOLD)),
            Span::styled("Confirm   ", Style::default().fg(t.bright)),
            Span::styled("[n/Esc] ", Style::default().fg(t.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(t.bright)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.panel_bg)),
        inner,
    );
}

pub(super) fn render_quit_confirm_dialog(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let active = app.active_agent_count();

    let popup_width = 58u16.min(area.width.saturating_sub(4));
    let popup_height = 7u16.min(area.height.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(t.danger))
        .title(Span::styled(
            " ⚠ CONFIRM QUIT ",
            Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let agent_word = if active == 1 { "agent" } else { "agents" };
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {} {} still running. Quit?", active, agent_word),
                Style::default().fg(t.bright),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Active agents will be orphaned.",
                Style::default().fg(t.muted),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y/Enter] ", Style::default().fg(t.danger).add_modifier(Modifier::BOLD)),
            Span::styled("Quit      ", Style::default().fg(t.bright)),
            Span::styled("[n/Esc] ", Style::default().fg(t.primary).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(t.bright)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.panel_bg)),
        inner,
    );
}

pub(super) fn render_poll_error_banner(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let banner_height = if app.consecutive_poll_failures >= 3 { 2u16 } else { 1u16 };
    let banner_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: banner_height.min(area.height),
    };

    f.render_widget(Clear, banner_area);

    let error_text = app
        .last_poll_error
        .as_deref()
        .unwrap_or("bd CLI unavailable");

    let truncated_error = truncate_str(error_text, area.width.saturating_sub(28) as usize);

    let first_line = Line::from(vec![
        Span::styled(
            " ✗ POLL ERROR ",
            Style::default()
                .fg(Color::White)
                .bg(t.danger)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" #{} ", app.consecutive_poll_failures),
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(180, 20, 20))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", truncated_error),
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(120, 10, 10)),
        ),
    ]);

    let first_row = Rect {
        x: banner_area.x,
        y: banner_area.y,
        width: banner_area.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(first_line).style(Style::default().bg(Color::Rgb(120, 10, 10))),
        first_row,
    );

    if app.consecutive_poll_failures >= 3 && banner_height >= 2 {
        let second_row = Rect {
            x: banner_area.x,
            y: banner_area.y + 1,
            width: banner_area.width,
            height: 1,
        };
        let urgent_line = Line::from(Span::styled(
            " ⚠  Multiple failures — verify dolt server is running and bd is in PATH",
            Style::default()
                .fg(t.warn)
                .bg(Color::Rgb(40, 15, 0))
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(
            Paragraph::new(urgent_line).style(Style::default().bg(Color::Rgb(40, 15, 0))),
            second_row,
        );
    }
}

pub(super) fn render_search_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let match_info = if app.search_query.is_empty() {
        String::new()
    } else if app.search_matches.is_empty() {
        " [no matches]".to_string()
    } else {
        format!(
            " [{}/{}]  n/N: next/prev",
            app.search_current_idx + 1,
            app.search_matches.len()
        )
    };

    let line = Line::from(vec![
        Span::styled(
            " / ",
            Style::default().fg(t.warn).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.search_query.as_str(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(t.warn)),
        Span::styled(
            match_info.as_str(),
            Style::default().fg(
                if app.search_matches.is_empty() && !app.search_query.is_empty() {
                    t.danger
                } else {
                    t.muted
                },
            ),
        ),
        Span::styled("  [Esc] close", Style::default().fg(t.muted)),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 28))),
        area,
    );
}

pub(super) fn render_jump_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let matches = app.jump_matches();
    let match_info = if app.jump_query.is_empty() {
        " type issue ID...".to_string()
    } else if matches.is_empty() {
        " [no matches]".to_string()
    } else {
        let labels: Vec<String> = matches
            .iter()
            .take(5)
            .map(|(id, is_agent): &(String, bool)| {
                if *is_agent {
                    format!("{} (agent)", id)
                } else {
                    id.clone()
                }
            })
            .collect();
        let suffix = if matches.len() > 5 {
            format!(" +{} more", matches.len() - 5)
        } else {
            String::new()
        };
        format!(" [{}{}]", labels.join(", "), suffix)
    };

    let line = Line::from(vec![
        Span::styled(
            " / ",
            Style::default().fg(t.info).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.jump_query.as_str(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(t.info)),
        Span::styled(
            match_info.as_str(),
            Style::default().fg(
                if matches.is_empty() && !app.jump_query.is_empty() {
                    t.danger
                } else {
                    t.muted
                },
            ),
        ),
        Span::styled("  Enter: jump  Esc: cancel", Style::default().fg(t.muted)),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 28))),
        area,
    );
}
