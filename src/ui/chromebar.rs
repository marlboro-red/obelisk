use super::*;

pub(super) fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let blink = (app.frame_count / 5).is_multiple_of(2);
    let dot = if blink { "●" } else { "○" };

    let title = Line::from(vec![
        Span::styled(
            " OBELISK",
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " / ",
            Style::default().fg(t.muted),
        ),
        Span::styled(
            "BEADS ORCHESTRATOR",
            Style::default().fg(t.muted),
        ),
        Span::styled(
            "  ◈ ",
            Style::default().fg(t.muted),
        ),
        Span::styled(
            app.repo_name.to_uppercase(),
            Style::default()
                .fg(t.info)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(format!("{} ", dot), Style::default().fg(t.accent)),
        Span::styled(
            "ONLINE",
            Style::default()
                .fg(t.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .style(Style::default().bg(t.dark_bg));

    let paragraph = Paragraph::new(title)
        .block(block)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

pub(super) fn render_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    // Compute badge counts
    let ready_count = app.ready_tasks.len();
    let active_count = app.agents.iter().filter(|a| {
        a.status == AgentStatus::Running || a.status == AgentStatus::Starting || a.status == AgentStatus::Killing
    }).count();
    let unread_events = app.event_log.len().saturating_sub(app.event_log_seen_count);
    let history_count = app.history_sessions.len();

    // Build tab titles with badges
    let dashboard_badge = if ready_count > 0 || active_count > 0 {
        format!(" ({}/{}) ", ready_count, active_count)
    } else {
        String::new()
    };
    let event_badge = if unread_events > 0 {
        format!(" ({}) ", unread_events)
    } else {
        String::new()
    };
    let history_badge = if history_count > 0 {
        format!(" ({}) ", history_count)
    } else {
        String::new()
    };

    let badge_style = Style::default().fg(t.muted);

    let tab_titles = vec![
        Line::from(vec![
            Span::raw(" 1:DASHBOARD"),
            Span::styled(dashboard_badge, badge_style),
        ]),
        Line::from(" 2:AGENTS "),
        Line::from(vec![
            Span::raw(" 3:EVENT LOG"),
            Span::styled(event_badge, badge_style),
        ]),
        Line::from(vec![
            Span::raw(" 4:HISTORY"),
            Span::styled(history_badge, badge_style),
        ]),
        Line::from(" 5:SPLIT "),
        Line::from(" 6:WORKTREES "),
        Line::from(" 7:DEPS "),
    ];

    let selected = match app.active_view {
        View::Dashboard => 0,
        View::AgentDetail => 1,
        View::EventLog => 2,
        View::History => 3,
        View::SplitPane => 4,
        View::WorktreeOverview => 5,
        View::DepGraph => 6,
    };

    let tabs = Tabs::new(tab_titles)
        .select(selected)
        .style(Style::default().fg(t.muted).bg(t.dark_bg))
        .highlight_style(
            Style::default()
                .fg(t.primary)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(Span::styled(" │ ", Style::default().fg(t.muted)));

    f.render_widget(tabs, area);
}

pub(super) fn render_status_summary(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let running = app.count_running();
    let queued = app.ready_tasks.len();
    let done = app.count_completed();
    let failed = app.count_failed();

    let running_style = if running > 0 {
        Style::default()
            .fg(t.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.muted)
    };

    let queued_style = if queued > 0 {
        Style::default()
            .fg(t.warn)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.muted)
    };

    let done_style = if done > 0 {
        Style::default().fg(t.info)
    } else {
        Style::default().fg(t.muted)
    };

    let failed_style = if failed > 0 {
        Style::default()
            .fg(t.danger)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.muted)
    };

    let line = Line::from(vec![
        Span::styled("  ▶ ", running_style),
        Span::styled(format!("{} running", running), running_style),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("◆ ", queued_style),
        Span::styled(format!("{} queued", queued), queued_style),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("✓ ", done_style),
        Span::styled(format!("{} done", done), done_style),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("✗ ", failed_style),
        Span::styled(format!("{} failed", failed), failed_style),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(t.dark_bg)),
        area,
    );
}

pub(super) fn render_status_gauges(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Completion rate gauge
    let rate = app.completion_rate();
    let rate_color = if rate >= 80.0 {
        t.accent
    } else if rate >= 50.0 {
        t.warn
    } else if rate > 0.0 {
        t.primary
    } else {
        t.muted
    };

    let rate_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(rate_color))
                .title(Span::styled(
                    " COMPLETION ",
                    Style::default()
                        .fg(rate_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(t.panel_bg)),
        )
        .gauge_style(Style::default().fg(rate_color).bg(Color::Rgb(25, 25, 35)))
        .ratio(rate / 100.0)
        .label(Span::styled(
            format!("{:.0}%", rate),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(rate_gauge, chunks[0]);

    // Poll countdown gauge
    let poll_ratio = if app.poll_interval_secs > 0 {
        app.poll_countdown / app.poll_interval_secs as f64
    } else {
        0.0
    }
    .clamp(0.0, 1.0);

    let poll_color = if !app.last_poll_ok {
        t.danger
    } else if poll_ratio > 0.5 {
        t.info
    } else if poll_ratio > 0.2 {
        t.warn
    } else {
        t.danger
    };

    let poll_title = if !app.last_poll_ok {
        format!(" POLL ERROR [{}x] ", app.consecutive_poll_failures)
    } else {
        " POLL CYCLE ".to_string()
    };

    let poll_label = if !app.last_poll_ok {
        "FAIL".to_string()
    } else {
        format!("{:.0}s", app.poll_countdown)
    };

    let poll_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(poll_color))
                .title(Span::styled(
                    poll_title,
                    Style::default()
                        .fg(poll_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(t.panel_bg)),
        )
        .gauge_style(Style::default().fg(poll_color).bg(Color::Rgb(25, 25, 35)))
        .ratio(poll_ratio)
        .label(Span::styled(
            poll_label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(poll_gauge, chunks[1]);
}

pub(super) fn render_info_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let blink = (app.frame_count / 8).is_multiple_of(2);

    let auto_style = if app.auto_spawn {
        if blink {
            Style::default()
                .fg(t.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim_accent)
        }
    } else {
        Style::default().fg(t.muted)
    };

    let model_name = app.selected_model();
    // Show short model name: strip common prefixes for display
    let model_short = model_name
        .strip_prefix("claude-")
        .or_else(|| model_name.strip_prefix("gpt-"))
        .unwrap_or(model_name);

    let line = Line::from(vec![
        Span::styled("  RUNTIME: ", Style::default().fg(t.muted)),
        Span::styled(
            app.selected_runtime.name(),
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  MODEL: ", Style::default().fg(t.muted)),
        Span::styled(
            model_short,
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("AGENTS: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}/{}", app.active_agent_count(), app.max_concurrent),
            Style::default().fg(t.bright),
        ),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("COMPLETED: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", app.total_completed),
            Style::default().fg(t.accent),
        ),
        Span::styled("  FAILED: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", app.total_failed),
            Style::default().fg(if app.total_failed > 0 { t.danger } else { t.muted }),
        ),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("AUTO: ", Style::default().fg(t.muted)),
        Span::styled(
            if app.auto_spawn { "ON" } else { "OFF" },
            auto_style,
        ),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("QUEUE: ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", app.ready_tasks.len()),
            Style::default().fg(if app.ready_tasks.is_empty() {
                t.muted
            } else {
                t.warn
            }),
        ),
        Span::styled("  │  ", Style::default().fg(t.muted)),
        Span::styled("NOTIFY: ", Style::default().fg(t.muted)),
        Span::styled(
            if app.notifications_enabled { "ON" } else { "OFF" },
            if app.notifications_enabled {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            },
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .style(Style::default().bg(t.panel_bg));

    f.render_widget(Paragraph::new(line).block(block), area);
}

/// Compact single-line info bar for terminals with < 40 rows.
/// Shows the most important stats without a border box.
pub(super) fn render_info_bar_compact(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let runtime_color = t.bright;

    let auto_style = if app.auto_spawn {
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.muted)
    };

    let line = Line::from(vec![
        Span::styled(" ", Style::default().fg(t.muted)),
        Span::styled(
            app.selected_runtime.name(),
            Style::default().fg(runtime_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}/{}", app.active_agent_count(), app.max_concurrent),
            Style::default().fg(t.bright),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}\u{2713}", app.total_completed),
            Style::default().fg(t.accent),
        ),
        Span::styled(" ", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}\u{2717}", app.total_failed),
            Style::default().fg(if app.total_failed > 0 { t.danger } else { t.muted }),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(t.muted)),
        Span::styled(
            if app.auto_spawn { "AUTO" } else { "MANUAL" },
            auto_style,
        ),
        Span::styled(" \u{2502} Q:", Style::default().fg(t.muted)),
        Span::styled(
            format!("{}", app.ready_tasks.len()),
            Style::default().fg(if app.ready_tasks.is_empty() { t.muted } else { t.warn }),
        ),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(t.panel_bg)),
        area,
    );
}

pub(super) fn render_keybindings(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let keys = match app.active_view {
        View::Dashboard => if app.focus == Focus::AgentList {
            vec![
                ("f", "filter"),
                ("Tab", "focus"),
                ("j/k", "nav"),
                ("Enter", "detail"),
                ("x", "dismiss"),
                ("X", "dismiss all"),
                ("+/-", "slots"),
                ("w", "worktrees"),
                ("1-7", "view"),
                ("?", "help"),
                ("q", "quit"),
            ]
        } else {
            vec![
                ("s", "spawn"),
                ("p", "poll"),
                ("c", "cleanup"),
                ("r", "runtime"),
                ("m", "model"),
                ("a", "auto"),
                ("n", "notify"),
                ("f", "sort"),
                ("F", "filter"),
                ("Tab", "focus"),
                ("j/k", "nav"),
                ("w", "worktrees"),
                ("+/-", "slots"),
                ("1-7", "view"),
                ("?", "help"),
                ("q", "quit"),
            ]
        },
        View::AgentDetail => if app.interactive_mode {
            vec![
                ("F2", "detach"),
                ("", "— all other keys forwarded to agent PTY —"),
            ]
        } else if app.search_active {
            vec![
                ("type", "search query"),
                ("n/N", "next/prev match"),
                ("Esc", "close search"),
            ]
        } else {
            let mut keys = vec![
                ("i", "interact"),
                ("d", "diff"),
                ("e", "export"),
                ("r", "retry"),
                ("↑↓", "scroll"),
            ];
            if app.show_diff_panel {
                keys.push(("J/K", "diff scroll"));
            }
            keys.extend([
                ("PgUp/Dn", "page"),
                ("←→", "prev/next"),
                ("/", "search"),
                ("Esc", "back"),
                ("D", "done"),
                ("k", "kill"),
                ("?", "help"),
                ("q", "back"),
            ]);
            keys
        },
        View::EventLog => vec![
            ("↑↓", "scroll"),
            ("1-7", "view"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::History => vec![
            ("↑↓", "scroll"),
            ("PgUp/Dn", "page"),
            ("1-7", "view"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::SplitPane => vec![
            ("Tab", "focus pane"),
            ("↑↓", "scroll"),
            ("Enter", "detail"),
            ("g", "pin/unpin"),
            ("1-7", "view"),
            ("?", "help"),
            ("Esc/q", "back"),
        ],
        View::WorktreeOverview => vec![
            ("↑↓/j/k", "nav"),
            ("f", "sort"),
            ("1-7", "view"),
            ("?", "help"),
            ("Esc/q", "back"),
        ],
        View::DepGraph => vec![
            ("↑↓/j/k", "nav"),
            ("Enter", "expand/collapse"),
            ("1-7", "view"),
            ("?", "help"),
            ("Esc/q", "back"),
        ],
    };

    let spans: Vec<Span> = keys
        .iter()
        .flat_map(|(key, action)| {
            vec![
                Span::styled(
                    format!(" [{}]", key),
                    Style::default()
                        .fg(t.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", action), Style::default().fg(t.muted)),
            ]
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(t.dark_bg));
    f.render_widget(paragraph, area);
}
