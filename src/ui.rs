use crate::app::{self, App};
use crate::types::*;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Tabs, Wrap,
    },
    Frame,
};

// ══════════════════════════════════════════════════════════
//  COLOR PALETTE
// ══════════════════════════════════════════════════════════

const PRIMARY: Color = Color::Rgb(255, 103, 0);    // Orange
const ACCENT: Color = Color::Rgb(0, 255, 65);      // Green
const SECONDARY: Color = Color::Rgb(148, 0, 211);  // Purple
const DANGER: Color = Color::Rgb(255, 40, 40);     // Red
const INFO: Color = Color::Rgb(0, 160, 255);       // Blue
const WARN: Color = Color::Rgb(255, 191, 0);       // Amber
const DARK_BG: Color = Color::Rgb(5, 5, 10);
const PANEL_BG: Color = Color::Rgb(10, 10, 18);
const MUTED: Color = Color::Rgb(70, 70, 90);
const BRIGHT: Color = Color::Rgb(200, 200, 220);
const DIM_ACCENT: Color = Color::Rgb(0, 120, 40);
fn primary_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PRIMARY))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG))
}

// ══════════════════════════════════════════════════════════
//  MAIN RENDER
// ══════════════════════════════════════════════════════════

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    // Clear all cells first to prevent artifacts when switching views
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(DARK_BG)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title bar
            Constraint::Length(1), // Tab bar
            Constraint::Min(10),  // Main content
            Constraint::Length(3), // Status gauges
            Constraint::Length(3), // Info bar
            Constraint::Length(1), // Keybindings
        ])
        .split(area);

    render_title_bar(f, chunks[0], app);
    render_tab_bar(f, chunks[1], app);

    match app.active_view {
        View::Dashboard => render_dashboard(f, chunks[2], app),
        View::AgentDetail => render_agent_detail(f, chunks[2], app),
        View::EventLog => render_event_log(f, chunks[2], app),
        View::History => render_history(f, chunks[2], app),
        View::SplitPane => render_split_pane(f, chunks[2], app),
    }

    render_status_gauges(f, chunks[3], app);
    render_info_bar(f, chunks[4], app);
    render_keybindings(f, chunks[5], app);

    // Persistent poll-error banner — rendered on top of main content
    if !app.last_poll_ok {
        render_poll_error_banner(f, chunks[2], app);
    }

    if app.show_help {
        render_help_overlay(f, area);
    }
}

// ══════════════════════════════════════════════════════════
//  TITLE BAR
// ══════════════════════════════════════════════════════════

fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let blink = (app.frame_count / 5) % 2 == 0;
    let dot = if blink { "●" } else { "○" };

    let title = Line::from(vec![
        Span::styled(
            " ◈ ",
            Style::default()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "OBELISK",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ◈ ",
            Style::default()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "BEADS ORCHESTRATOR",
            Style::default()
                .fg(BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  //  ", Style::default().fg(MUTED)),
        Span::styled(format!("{} ", dot), Style::default().fg(ACCENT)),
        Span::styled(
            "ONLINE",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PRIMARY))
        .style(Style::default().bg(DARK_BG));

    let paragraph = Paragraph::new(title)
        .block(block)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

// ══════════════════════════════════════════════════════════
//  TAB BAR
// ══════════════════════════════════════════════════════════

fn render_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab_titles = vec![
        Line::from(" 1:DASHBOARD "),
        Line::from(" 2:AGENTS "),
        Line::from(" 3:EVENT LOG "),
        Line::from(" 4:HISTORY "),
        Line::from(" 5:SPLIT "),
    ];

    let selected = match app.active_view {
        View::Dashboard => 0,
        View::AgentDetail => 1,
        View::EventLog => 2,
        View::History => 3,
        View::SplitPane => 4,
    };

    let tabs = Tabs::new(tab_titles)
        .select(selected)
        .style(Style::default().fg(MUTED).bg(DARK_BG))
        .highlight_style(
            Style::default()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(Span::styled(" │ ", Style::default().fg(MUTED)));

    f.render_widget(tabs, area);
}

// ══════════════════════════════════════════════════════════
//  DASHBOARD VIEW
// ══════════════════════════════════════════════════════════

fn render_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // Top: ready queue + agents
            Constraint::Length(5), // Bottom: throughput + mini log
        ])
        .split(area);

    // Alert banner overlay
    if let Some((ref msg, _)) = app.alert_message {
        let blink = (app.frame_count / 3) % 2 == 0;
        if blink {
            let alert_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            };
            let alert = Paragraph::new(Line::from(Span::styled(
                msg.as_str(),
                Style::default()
                    .fg(Color::White)
                    .bg(DANGER)
                    .add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center);
            f.render_widget(alert, alert_area);
        }
    }

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(v_chunks[0]);

    // Split left column: queue list on top, task detail preview on bottom
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(9)])
        .split(h_chunks[0]);

    render_ready_queue(f, left_chunks[0], app);
    render_task_preview(f, left_chunks[1], app);
    render_agent_panel(f, h_chunks[1], app);

    // Bottom: throughput sparkline + mini event log
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(v_chunks[1]);

    render_throughput_sparkline(f, bottom_chunks[0], app);
    render_mini_event_log(f, bottom_chunks[1], app);
}

fn render_throughput_sparkline(f: &mut Frame, area: Rect, app: &App) {
    let data: Vec<u64> = app
        .throughput_history
        .iter()
        .map(|&v| v as u64)
        .collect();

    let max_val = data.iter().copied().max().unwrap_or(1).max(1);
    let label = format!("peak: {}/s", max_val);

    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SECONDARY))
                .title(Span::styled(
                    " THROUGHPUT ",
                    Style::default()
                        .fg(SECONDARY)
                        .add_modifier(Modifier::BOLD),
                ))
                .title_bottom(Line::from(Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(MUTED),
                )))
                .style(Style::default().bg(PANEL_BG)),
        )
        .data(&data)
        .style(Style::default().fg(ACCENT));

    f.render_widget(sparkline, area);
}

fn render_mini_event_log(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(
            " RECENT EVENTS ",
            Style::default()
                .fg(WARN)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let inner = block.inner(area);
    let visible = inner.height as usize;

    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .take(visible)
        .map(|entry| {
            let cat_color = log_category_color(entry.category);

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", entry.timestamp), Style::default().fg(MUTED)),
                Span::styled(
                    format!("[{}] ", entry.category.label()),
                    Style::default().fg(cat_color),
                ),
                Span::styled(
                    truncate_str(&entry.message, 40),
                    Style::default().fg(BRIGHT),
                ),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(block), area);
}

fn render_ready_queue(f: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focus == Focus::ReadyQueue && app.active_view == View::Dashboard;
    let border_color = if is_focused { ACCENT } else { MUTED };

    let filtered = app.filtered_tasks();
    let total = app.ready_tasks.len();
    let shown = filtered.len();

    // Build the sort/filter indicator suffixes for the title
    let sort_label = format!("[sort: {}]", app.sort_mode.label());
    let filter_label = if app.type_filter.is_empty() {
        String::new()
    } else {
        let mut types: Vec<&str> = app.type_filter.iter().map(|s| s.as_str()).collect();
        types.sort_unstable();
        format!(" [filter: {}]", types.join(","))
    };
    let count_label = if shown == total {
        format!("{}", total)
    } else {
        format!("{}/{}", shown, total)
    };
    let title = format!(
        "◆ READY QUEUE [{}] {} {}",
        count_label, sort_label, filter_label
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Rounded
        })
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title.trim_end()),
            Style::default()
                .fg(if is_focused { ACCENT } else { MUTED })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    if filtered.is_empty() {
        let empty_msg = if app.ready_tasks.is_empty() {
            Line::from(vec![
                Span::styled("  No ready tasks — ", Style::default().fg(MUTED)),
                Span::styled("STANDBY", Style::default().fg(WARN)),
            ])
        } else {
            Line::from(vec![
                Span::styled("  No tasks match filter — ", Style::default().fg(MUTED)),
                Span::styled("press F to change", Style::default().fg(WARN)),
            ])
        };
        let empty = Paragraph::new(empty_msg).block(block);
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let priority = task.priority.unwrap_or(3);
            let p_style = match priority {
                0 => Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
                1 => Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
                2 => Style::default().fg(WARN),
                3 => Style::default().fg(BRIGHT),
                _ => Style::default().fg(MUTED),
            };

            let issue_type = task.issue_type.as_deref().unwrap_or("task");
            let type_str = match issue_type {
                "bug" => "BUG",
                "feature" => "FTR",
                "task" => "TSK",
                "epic" => "EPC",
                "chore" => "CHR",
                _ => "???",
            };

            let sel_indicator = if Some(i) == app.task_list_state.selected() && is_focused {
                Span::styled(
                    " ▸ ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            ListItem::new(Line::from(vec![
                sel_indicator,
                Span::styled(format!("P{} ", priority), p_style),
                Span::styled(format!("[{}] ", type_str), Style::default().fg(INFO)),
                Span::styled(format!("{}: ", task.id), Style::default().fg(SECONDARY)),
                Span::styled(truncate_str(&task.title, 30), Style::default().fg(BRIGHT)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Rgb(20, 30, 20))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut app.task_list_state.clone());
}

fn render_task_preview(f: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focus == Focus::ReadyQueue && app.active_view == View::Dashboard;
    let border_color = if is_focused { INFO } else { MUTED };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " ◆ DETAIL ",
            Style::default().fg(INFO).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let task = match app.selected_task() {
        Some(t) => t,
        None => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    " Select a task to view details",
                    Style::default().fg(MUTED),
                ))
                .block(block),
                area,
            );
            return;
        }
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let priority = task.priority.unwrap_or(3);
    let p_color = match priority {
        0 => DANGER,
        1 => PRIMARY,
        2 => WARN,
        _ => BRIGHT,
    };

    let type_str = match task.issue_type.as_deref().unwrap_or("task") {
        "bug" => "BUG",
        "feature" => "FTR",
        "task" => "TSK",
        "epic" => "EPC",
        "chore" => "CHR",
        _ => "???",
    };

    let description = task.description.as_deref().unwrap_or("(no description)");
    let labels_str = task.labels.as_ref().map(|l| l.join(", ")).unwrap_or_default();
    let assignee = task.assignee.as_deref().unwrap_or("");

    let mut meta_spans = vec![
        Span::styled(format!("[{}] ", type_str), Style::default().fg(INFO)),
        Span::styled(format!("P{}  ", priority), Style::default().fg(p_color)),
    ];
    if !assignee.is_empty() {
        meta_spans.push(Span::styled(assignee, Style::default().fg(SECONDARY)));
    }
    if !labels_str.is_empty() {
        meta_spans.push(Span::styled(
            format!("  ◦ {}", labels_str),
            Style::default().fg(MUTED),
        ));
    }

    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            task.title.as_str(),
            Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD),
        )),
        Line::from(meta_spans),
        Line::from(""),
        Line::from(Span::styled(description, Style::default().fg(MUTED))),
    ];

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_agent_panel(f: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focus == Focus::AgentList && app.active_view == View::Dashboard;
    let border_color = if is_focused { ACCENT } else { MUTED };

    let active = app.active_agent_count();
    let total = app.agents.len();
    let visible_agents = app.filtered_agents();
    let visible_count = visible_agents.len();

    // Build title: count section + optional filter badge
    let count_part = if app.agent_status_filter == AgentStatusFilter::All {
        format!("◆ ACTIVE AGENTS [{}/{}]", active, app.max_concurrent)
    } else {
        format!(
            "◆ ACTIVE AGENTS [{}/{}] [{}/{}]",
            active,
            app.max_concurrent,
            visible_count,
            total,
        )
    };
    let filter_badge = if app.agent_status_filter != AgentStatusFilter::All {
        format!(" [{}]", app.agent_status_filter.label())
    } else {
        String::new()
    };
    let title = format!("{}{}", count_part, filter_badge);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Rounded
        })
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(if is_focused { ACCENT } else { MUTED })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    if app.agents.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::styled("  No agents deployed — ", Style::default().fg(MUTED)),
            Span::styled("IDLE", Style::default().fg(WARN)),
        ]))
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    if visible_agents.is_empty() {
        // Filter active but no agents match
        let msg = format!(
            "  No {} agents",
            app.agent_status_filter.label()
        );
        let empty = Paragraph::new(Line::from(vec![
            Span::styled(msg, Style::default().fg(MUTED)),
        ]))
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = visible_agents
        .iter()
        .enumerate()
        .map(|(i, (_raw_idx, agent))| {
            let status_style = match agent.status {
                AgentStatus::Starting => Style::default().fg(WARN),
                AgentStatus::Running => Style::default().fg(ACCENT),
                AgentStatus::Completed => Style::default().fg(INFO),
                AgentStatus::Failed => Style::default().fg(DANGER),
            };

            let runtime_style = match agent.runtime {
                Runtime::ClaudeCode => Style::default().fg(PRIMARY),
                Runtime::Codex => Style::default().fg(ACCENT),
                Runtime::Copilot => Style::default().fg(INFO),
            };

            let elapsed = App::format_elapsed(agent.elapsed_secs);

            let sel_indicator = if Some(i) == app.agent_list_state.selected() && is_focused {
                Span::styled(
                    " ▸ ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            let status_text = match agent.status {
                AgentStatus::Starting => "INIT",
                AgentStatus::Running => &elapsed,
                AgentStatus::Completed => "DONE",
                AgentStatus::Failed => "FAIL",
            };

            let line_count = app.agent_line_count(agent.id);

            let phase_badge = if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
                Span::styled(
                    format!(" [{}]", agent.phase.short()),
                    Style::default()
                        .fg(phase_color(agent.phase))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("", Style::default())
            };

            ListItem::new(Line::from(vec![
                sel_indicator,
                Span::styled(agent.status.symbol(), status_style),
                Span::styled(
                    format!(" AGENT-{:02} ", agent.unit_number),
                    Style::default()
                        .fg(SECONDARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", agent.task.id), Style::default().fg(BRIGHT)),
                Span::styled(format!("[{}] ", agent.runtime.name()), runtime_style),
                Span::styled(status_text.to_string(), status_style),
                phase_badge,
                Span::styled(
                    format!("  ({} lines)", line_count),
                    Style::default().fg(MUTED),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Rgb(20, 20, 30))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut app.agent_list_state.clone());
}

// ══════════════════════════════════════════════════════════
//  AGENT DETAIL VIEW
// ══════════════════════════════════════════════════════════

fn render_agent_detail(f: &mut Frame, area: Rect, app: &App) {
    let agent = app
        .selected_agent_id
        .and_then(|id| app.agents.iter().find(|a| a.id == id));

    let agent = match agent {
        Some(a) => a,
        None => {
            let block = primary_block("NO AGENT SELECTED");
            let p = Paragraph::new("Press ESC to return to dashboard")
                .block(block)
                .style(Style::default().fg(MUTED));
            f.render_widget(p, area);
            return;
        }
    };

    let status_str = match agent.status {
        AgentStatus::Starting => "INITIALIZING",
        AgentStatus::Running => "ACTIVE",
        AgentStatus::Completed => "COMPLETE",
        AgentStatus::Failed => "TERMINATED",
    };

    let header_color = match agent.status {
        AgentStatus::Starting => WARN,
        AgentStatus::Running => ACCENT,
        AgentStatus::Completed => INFO,
        AgentStatus::Failed => DANGER,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Agent header
            Constraint::Min(5),   // Output
        ])
        .split(area);

    // Agent header
    let mut header_spans = vec![
        Span::styled(
            format!("  AGENT-{:02}", agent.unit_number),
            Style::default()
                .fg(SECONDARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  //  ", Style::default().fg(MUTED)),
        Span::styled(agent.task.id.as_str(), Style::default().fg(BRIGHT)),
        Span::styled(
            format!("  [{}]", agent.runtime.name()),
            Style::default().fg(PRIMARY),
        ),
        Span::styled(
            format!("  {}", agent.model),
            Style::default().fg(WARN),
        ),
        Span::styled("  //  ", Style::default().fg(MUTED)),
        Span::styled(
            status_str,
            Style::default()
                .fg(header_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  //  ", Style::default().fg(MUTED)),
        Span::styled(
            App::format_elapsed(agent.elapsed_secs),
            Style::default().fg(WARN),
        ),
    ];
    if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
        header_spans.push(Span::styled("  //  ", Style::default().fg(MUTED)));
        header_spans.extend(render_phase_indicator(agent.phase));
    }
    let header_line = Line::from(header_spans);

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(header_color))
        .title(Span::styled(
            " AGENT TELEMETRY ",
            Style::default()
                .fg(header_color)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    f.render_widget(
        Paragraph::new(header_line).block(header_block),
        chunks[0],
    );

    // Split output area: output on left, stats/diff on right
    let output_chunks = if app.show_diff_panel {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(28)])
            .split(chunks[1])
    };

    // Output area — use PseudoTerminal widget if PTY is active, else legacy text view
    let mode_label = if app.interactive_mode { "INTERACTIVE" } else { "OBSERVE" };
    let mode_color = if app.interactive_mode { ACCENT } else { MUTED };
    let output_title = format!("TERMINAL [{}]", mode_label);

    let output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(if app.interactive_mode {
            BorderType::Double
        } else {
            BorderType::Rounded
        })
        .border_style(Style::default().fg(if app.interactive_mode { ACCENT } else { SECONDARY }))
        .title(Span::styled(
            format!(" {} ", output_title),
            Style::default()
                .fg(mode_color)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    if let Some(pty_state) = app.pty_states.get(&agent.id) {
        // ── PTY mode: render full terminal emulation ──
        let inner = output_block.inner(output_chunks[0]);
        f.render_widget(output_block, output_chunks[0]);
        if app.search_active && inner.height > 1 {
            let vt100_area = Rect { height: inner.height - 1, ..inner };
            let search_bar_area = Rect { y: inner.y + inner.height - 1, height: 1, ..inner };
            render_vt100_screen(f, pty_state.parser.screen(), vt100_area, app);
            render_search_bar(f, search_bar_area, app);
        } else {
            render_vt100_screen(f, pty_state.parser.screen(), inner, app);
        }
    } else {
        // ── Legacy fallback: line-based output ──
        let inner = output_block.inner(output_chunks[0]);
        let visible_height = inner.height as usize;
        let total_lines = agent.output.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll = match app.agent_output_scroll {
            None => max_scroll,
            Some(pos) => pos.min(max_scroll),
        };

        let lines: Vec<Line> = agent
            .output
            .iter()
            .skip(scroll)
            .take(visible_height)
            .enumerate()
            .map(|(i, line)| {
                let line_num = scroll + i + 1;
                Line::from(vec![
                    Span::styled(format!("{:4} │ ", line_num), Style::default().fg(MUTED)),
                    Span::styled(line.as_str(), Style::default().fg(ACCENT)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(output_block);
        f.render_widget(paragraph, output_chunks[0]);

        if total_lines > visible_height {
            let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(SECONDARY)),
                output_chunks[0],
                &mut scrollbar_state,
            );
        }
    }

    // Right panel: diff or stats
    if app.show_diff_panel {
        render_diff_panel(f, output_chunks[1], app);
    } else {
        render_agent_stats(f, output_chunks[1], agent, app);
    }
}


// ══════════════════════════════════════════════════════════
//  SPLIT-PANE VIEW
// ══════════════════════════════════════════════════════════

fn render_split_pane(f: &mut Frame, area: Rect, app: &App) {
    let pane_count = app.split_pane_count(area.width);

    if pane_count <= 1 {
        let block = primary_block("SPLIT VIEW — terminal too narrow for multi-pane");
        let p = Paragraph::new("Widen your terminal to at least 80 columns for 2-up, or 160 for 4-up")
            .block(block)
            .style(Style::default().fg(MUTED));
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

                let status_color = match agent.status {
                    AgentStatus::Starting => WARN,
                    AgentStatus::Running => ACCENT,
                    AgentStatus::Completed => INFO,
                    AgentStatus::Failed => DANGER,
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

                let border_color = if is_focused { PRIMARY } else { MUTED };
                let border_type = if is_focused {
                    BorderType::Double
                } else {
                    BorderType::Rounded
                };

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(border_type)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().bg(PANEL_BG));

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
                        .map(|line| {
                            Line::from(Span::styled(line.as_str(), Style::default().fg(ACCENT)))
                        })
                        .collect();

                    let paragraph = Paragraph::new(lines);
                    f.render_widget(paragraph, inner);
                }
            }
            None => {
                let border_color = if is_focused { PRIMARY } else { MUTED };
                let border_type = if is_focused {
                    BorderType::Double
                } else {
                    BorderType::Rounded
                };

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(border_type)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        format!(" PANE {} — empty ", slot + 1),
                        Style::default().fg(MUTED),
                    ))
                    .style(Style::default().bg(PANEL_BG));

                let p = Paragraph::new(Line::from(Span::styled(
                    "  No agent assigned",
                    Style::default().fg(MUTED),
                )))
                .block(block);
                f.render_widget(p, pane_rect);
            }
        }
    }
}

/// Render a vt100 screen without search highlighting (for split panes).
fn render_vt100_screen_plain(f: &mut Frame, screen: &vt100::Screen, area: Rect) {
    let buf = f.buffer_mut();
    let rows = area.height as usize;
    let cols = area.width as usize;
    let (screen_rows, screen_cols) = (screen.size().0 as usize, screen.size().1 as usize);

    for row in 0..rows.min(screen_rows) {
        for col in 0..cols.min(screen_cols) {
            let cell = screen.cell(row as u16, col as u16);
            let x = area.x + col as u16;
            let y = area.y + row as u16;

            if x >= area.x + area.width || y >= area.y + area.height {
                continue;
            }

            if let Some(cell) = cell {
                let fg = vt100_color_to_ratatui(cell.fgcolor());
                let bg = vt100_color_to_ratatui(cell.bgcolor());
                let mut style = Style::default().fg(fg).bg(bg);

                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                let contents = cell.contents();
                let ch = if contents.is_empty() {
                    ' '
                } else {
                    contents.chars().next().unwrap_or(' ')
                };

                buf[(x, y)].set_char(ch).set_style(style);
            }
        }
    }
}
fn render_agent_stats(f: &mut Frame, area: Rect, agent: &AgentInstance, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .title(Span::styled(
            " DIAGNOSTICS ",
            Style::default()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let total_lines = app.agent_line_count(agent.id);
    let elapsed = agent.elapsed_secs.max(1);
    let lines_per_sec = total_lines as f64 / elapsed as f64;

    let timeout_line = {
        let (timeout_text, timeout_color) = if app.agent_timeout_secs == 0 {
            (" DISABLED".to_string(), MUTED)
        } else if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
            (" --".to_string(), MUTED)
        } else {
            let limit = app.agent_timeout_secs;
            let warn_at = limit * 4 / 5;
            if agent.elapsed_secs >= limit {
                (" EXPIRED".to_string(), DANGER)
            } else {
                let remaining = limit.saturating_sub(agent.elapsed_secs);
                let text = format!(" in {}", App::format_elapsed(remaining));
                let color = if agent.elapsed_secs >= warn_at { WARN } else { BRIGHT };
                (text, color)
            }
        };
        Line::from(vec![
            Span::styled(" TIMEOUT ", Style::default().fg(MUTED)),
            Span::styled(timeout_text, Style::default().fg(timeout_color)),
        ])
    };

    let status_indicator = match agent.status {
        AgentStatus::Starting => ("◐ INITIALIZING", WARN),
        AgentStatus::Running => ("▶ ACTIVE", ACCENT),
        AgentStatus::Completed => ("✓ COMPLETE", INFO),
        AgentStatus::Failed => ("✗ TERMINATED", DANGER),
    };

    let hex_row = if (app.frame_count / 4) % 2 == 0 {
        "⬡ ⬢ ⬡ ⬢ ⬡ ⬢ ⬡ ⬢"
    } else {
        "⬢ ⬡ ⬢ ⬡ ⬢ ⬡ ⬢ ⬡"
    };

    let priority_str = agent
        .task
        .priority
        .map(|p| format!("P{}", p))
        .unwrap_or_else(|| "P?".into());

    let retry_color = if agent.retry_count >= app.max_retries {
        DANGER
    } else if agent.retry_count > 0 {
        WARN
    } else {
        MUTED
    };

    let worktree_line = agent.worktree_path.as_deref().map(|wt_path| {
        let (wt_label, wt_color) = if agent.worktree_cleaned {
            ("(cleaned)", MUTED)
        } else {
            match agent.status {
                AgentStatus::Starting | AgentStatus::Running => ("(active)", ACCENT),
                _ => ("(pending)", WARN),
            }
        };
        Line::from(vec![
            Span::styled(" WRKTR   ", Style::default().fg(MUTED)),
            Span::styled(format!(" {} ", wt_label), Style::default().fg(wt_color)),
            Span::styled(
                std::path::Path::new(wt_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(wt_path)
                    .to_string(),
                Style::default().fg(MUTED),
            ),
        ])
    });

    let mut lines = vec![
        Line::from(Span::styled(
            hex_row,
            Style::default().fg(Color::Rgb(30, 30, 50)),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" STATUS  ", Style::default().fg(MUTED)),
            Span::styled(
                status_indicator.0,
                Style::default()
                    .fg(status_indicator.1)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" RUNTIME ", Style::default().fg(MUTED)),
            Span::styled(
                agent.runtime.name(),
                Style::default()
                    .fg(PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" TASK    ", Style::default().fg(MUTED)),
            Span::styled(&agent.task.id, Style::default().fg(SECONDARY)),
        ]),
        Line::from(vec![
            Span::styled(" PRIORITY", Style::default().fg(MUTED)),
            Span::styled(format!(" {}", priority_str), Style::default().fg(WARN)),
        ]),
        Line::from(vec![
            Span::styled(" RETRIES ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {}/{}", agent.retry_count, app.max_retries),
                Style::default().fg(retry_color),
            ),
        ]),
        Line::from(vec![
            Span::styled(" PHASE   ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {} {}", agent.phase.short(), agent.phase.label()),
                Style::default()
                    .fg(phase_color(agent.phase))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" TMPL    ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {}", agent.template_name),
                Style::default().fg(SECONDARY),
            ),
        ]),
    ];
    if let Some(wl) = worktree_line {
        lines.push(wl);
    }
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled(" ELAPSED ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {}", App::format_elapsed(agent.elapsed_secs)),
                Style::default().fg(BRIGHT),
            ),
        ]),
        timeout_line,
        Line::from(vec![
            Span::styled(" LINES   ", Style::default().fg(MUTED)),
            Span::styled(format!(" {}", total_lines), Style::default().fg(BRIGHT)),
        ]),
        Line::from(vec![
            Span::styled(" RATE    ", Style::default().fg(MUTED)),
            Span::styled(format!(" {:.1}/s", lines_per_sec), Style::default().fg(ACCENT)),
        ]),
        Line::from(vec![
            Span::styled(" TOK IN  ", Style::default().fg(MUTED)),
            Span::styled(format!(" {}", app::format_tokens(agent.input_tokens)), Style::default().fg(INFO)),
        ]),
        Line::from(vec![
            Span::styled(" TOK OUT ", Style::default().fg(MUTED)),
            Span::styled(format!(" {}", app::format_tokens(agent.output_tokens)), Style::default().fg(INFO)),
        ]),
        Line::from(vec![
            Span::styled(" COST    ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {}", app::format_cost(agent.estimated_cost_usd)),
                Style::default().fg(if agent.estimated_cost_usd > 1.0 { WARN } else { ACCENT }),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            hex_row,
            Style::default().fg(Color::Rgb(30, 30, 50)),
        )),
    ]);

    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ══════════════════════════════════════════════════════════
//  DIFF PANEL
// ══════════════════════════════════════════════════════════

fn render_diff_panel(f: &mut Frame, area: Rect, app: &App) {
    let title = match &app.diff_data {
        Some(d) => format!(
            "GIT DIFF  {} file{}, +{} -{}",
            d.files_changed,
            if d.files_changed == 1 { "" } else { "s" },
            d.insertions,
            d.deletions,
        ),
        None => "GIT DIFF  loading...".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(INFO))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(INFO)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let inner = block.inner(area);
    let visible_height = inner.height as usize;

    let diff_data = match &app.diff_data {
        Some(d) => d,
        None => {
            // No data yet — show a loading indicator
            let loading = Paragraph::new(Line::from(Span::styled(
                "  Fetching diff...",
                Style::default().fg(MUTED),
            )))
            .block(block);
            f.render_widget(loading, area);
            return;
        }
    };

    if diff_data.lines.is_empty() {
        // No changes
        let no_changes = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No uncommitted changes",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Worktree is clean",
                Style::default().fg(DIM_ACCENT),
            )),
        ])
        .block(block);
        f.render_widget(no_changes, area);
        return;
    }

    // Build header: changed file list
    let mut lines: Vec<Line> = Vec::new();
    for file in &diff_data.changed_files {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("M ", Style::default().fg(WARN).add_modifier(Modifier::BOLD)),
            Span::styled(file.as_str(), Style::default().fg(BRIGHT)),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(inner.width.saturating_sub(0) as usize),
        Style::default().fg(MUTED),
    )));

    // Build diff lines with color coding
    for diff_line in &diff_data.lines {
        let (style, prefix) = if diff_line.starts_with('+') && !diff_line.starts_with("+++") {
            (Style::default().fg(ACCENT), "")
        } else if diff_line.starts_with('-') && !diff_line.starts_with("---") {
            (Style::default().fg(DANGER), "")
        } else if diff_line.starts_with("@@") {
            (Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD), "")
        } else if diff_line.starts_with("diff --git") {
            (Style::default().fg(INFO).add_modifier(Modifier::BOLD), "")
        } else if diff_line.starts_with("index ") || diff_line.starts_with("---") || diff_line.starts_with("+++") {
            (Style::default().fg(MUTED), "")
        } else {
            (Style::default().fg(Color::Rgb(140, 140, 160)), " ")
        };

        let display = format!("{}{}", prefix, diff_line);
        lines.push(Line::from(Span::styled(display, style)));
    }

    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.diff_scroll.min(max_scroll);

    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll).take(visible_height).collect();

    let paragraph = Paragraph::new(visible_lines).block(block);
    f.render_widget(paragraph, area);

    // Scrollbar
    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(INFO)),
            area,
            &mut scrollbar_state,
        );
    }
}

// ══════════════════════════════════════════════════════════
//  EVENT LOG VIEW
// ══════════════════════════════════════════════════════════

fn render_event_log(f: &mut Frame, area: Rect, app: &App) {
    let block = primary_block("◆ SYSTEM LOG");

    if app.event_log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No events recorded",
            Style::default().fg(MUTED),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let inner = block.inner(area);
    let visible = inner.height as usize;

    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .skip(app.log_scroll)
        .take(visible)
        .map(|entry| {
            let cat_color = log_category_color(entry.category);

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", entry.timestamp), Style::default().fg(MUTED)),
                Span::styled(
                    format!("[{}]", entry.category.label()),
                    Style::default()
                        .fg(cat_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", entry.message), Style::default().fg(BRIGHT)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

// ══════════════════════════════════════════════════════════
//  HISTORY VIEW
// ══════════════════════════════════════════════════════════


fn render_history(f: &mut Frame, area: Rect, app: &App) {
    let (total_sessions, all_completed, all_failed, avg_duration, all_time_cost) = app.aggregate_stats();
    let all_time_total = all_completed + all_failed;
    let success_rate = if all_time_total > 0 {
        all_completed as f64 / all_time_total as f64 * 100.0
    } else {
        0.0
    };

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Aggregate stats panel
            Constraint::Min(5),    // Session list
        ])
        .split(area);

    // ── Aggregate stats ──
    let stats_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PRIMARY))
        .title(Span::styled(
            " ◆ ALL-TIME STATISTICS ",
            Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let stats_lines = vec![
        Line::from(vec![
            Span::styled("  SESSIONS        ", Style::default().fg(MUTED)),
            Span::styled(format!("{}", total_sessions), Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  COMPLETED       ", Style::default().fg(MUTED)),
            Span::styled(format!("{}", all_completed), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  FAILED          ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{}", all_failed),
                if all_failed > 0 {
                    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("  SUCCESS RATE    ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{:.1}%", success_rate),
                if success_rate >= 80.0 {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else if success_rate >= 50.0 {
                    Style::default().fg(WARN).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("  AVG DURATION    ", Style::default().fg(MUTED)),
            Span::styled(
                App::format_elapsed(avg_duration as u64).to_string(),
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  TOTAL COST      ", Style::default().fg(MUTED)),
            Span::styled(
                app::format_cost(all_time_cost),
                Style::default().fg(if all_time_cost > 10.0 { WARN } else { ACCENT }).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(stats_lines).block(stats_block), v_chunks[0]);

    // ── Session list ──
    let sessions_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SECONDARY))
        .title(Span::styled(
            format!(" ◆ SESSION LOG [{}] ", app.history_sessions.len()),
            Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    if app.history_sessions.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::styled("  No sessions recorded yet — ", Style::default().fg(MUTED)),
            Span::styled("run some agents and quit to record your first session", Style::default().fg(WARN)),
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
            let rate_color = if rate >= 80.0 { ACCENT } else if rate >= 50.0 { WARN } else { DANGER };
            let short_id = if session.session_id.len() > 16 {
                &session.session_id[..16]
            } else {
                &session.session_id
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:16} ", short_id), Style::default().fg(SECONDARY)),
                Span::styled(format!("started: {:<22} ", &session.started_at[..session.started_at.len().min(19)]), Style::default().fg(MUTED)),
                Span::styled(format!("done:{:>4} ", session.total_completed), Style::default().fg(ACCENT)),
                Span::styled(format!("fail:{:>3} ", session.total_failed), Style::default().fg(if session.total_failed > 0 { DANGER } else { MUTED })),
                Span::styled(format!("{:>5.1}%", rate), Style::default().fg(rate_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {:>3} agents", session.agents.len()), Style::default().fg(MUTED)),
                Span::styled(format!("  {}", app::format_cost(session.total_cost_usd)), Style::default().fg(if session.total_cost_usd > 1.0 { WARN } else { ACCENT })),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(sessions_block), v_chunks[1]);
}

// ══════════════════════════════════════════════════════════
//  STATUS GAUGES
// ══════════════════════════════════════════════════════════

fn render_status_gauges(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(35),
            Constraint::Percentage(30),
        ])
        .split(area);

    // Completion rate gauge
    let rate = app.completion_rate();
    let rate_color = if rate >= 80.0 {
        ACCENT
    } else if rate >= 50.0 {
        WARN
    } else if rate > 0.0 {
        PRIMARY
    } else {
        MUTED
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
                .style(Style::default().bg(PANEL_BG)),
        )
        .gauge_style(Style::default().fg(rate_color).bg(Color::Rgb(20, 20, 30)))
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
        DANGER
    } else if poll_ratio > 0.5 {
        INFO
    } else if poll_ratio > 0.2 {
        WARN
    } else {
        DANGER
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
                .style(Style::default().bg(PANEL_BG)),
        )
        .gauge_style(Style::default().fg(poll_color).bg(Color::Rgb(20, 20, 30)))
        .ratio(poll_ratio)
        .label(Span::styled(
            poll_label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(poll_gauge, chunks[1]);

    // Wave pattern visualization
    render_wave_monitor(f, chunks[2], app);
}

fn render_wave_monitor(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SECONDARY))
        .title(Span::styled(
            " WAVEFORM ",
            Style::default()
                .fg(SECONDARY)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let inner = block.inner(area);
    let width = inner.width as usize;

    if width == 0 {
        f.render_widget(block, area);
        return;
    }

    let wave_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let has_active = app.active_agent_count() > 0;

    let wave: String = (0..width)
        .map(|x| {
            if has_active {
                let val = ((x as f64 * 0.5 + app.wave_offset).sin() * 0.5 + 0.5) * 7.0;
                wave_chars[val as usize % 8]
            } else {
                wave_chars[0]
            }
        })
        .collect();

    let wave_color = if has_active { ACCENT } else { MUTED };

    let paragraph = Paragraph::new(Line::from(Span::styled(
        wave,
        Style::default().fg(wave_color),
    )))
    .block(block);

    f.render_widget(paragraph, area);
}

// ══════════════════════════════════════════════════════════
//  INFO BAR
// ══════════════════════════════════════════════════════════

fn render_info_bar(f: &mut Frame, area: Rect, app: &App) {
    let blink = (app.frame_count / 8) % 2 == 0;

    let auto_style = if app.auto_spawn {
        if blink {
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM_ACCENT)
        }
    } else {
        Style::default().fg(MUTED)
    };

    let runtime_color = match app.selected_runtime {
        Runtime::ClaudeCode => PRIMARY,
        Runtime::Codex => ACCENT,
        Runtime::Copilot => INFO,
    };

    let model_name = app.selected_model();
    // Show short model name: strip common prefixes for display
    let model_short = model_name
        .strip_prefix("claude-")
        .or_else(|| model_name.strip_prefix("gpt-"))
        .unwrap_or(model_name);

    let line = Line::from(vec![
        Span::styled("  RUNTIME: ", Style::default().fg(MUTED)),
        Span::styled(
            app.selected_runtime.name(),
            Style::default()
                .fg(runtime_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  MODEL: ", Style::default().fg(MUTED)),
        Span::styled(
            model_short,
            Style::default()
                .fg(WARN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("AGENTS: ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{}/{}", app.active_agent_count(), app.max_concurrent),
            Style::default().fg(BRIGHT),
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("COMPLETED: ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{}", app.total_completed),
            Style::default().fg(ACCENT),
        ),
        Span::styled("  FAILED: ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{}", app.total_failed),
            Style::default().fg(if app.total_failed > 0 { DANGER } else { MUTED }),
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("AUTO: ", Style::default().fg(MUTED)),
        Span::styled(
            if app.auto_spawn { "ON" } else { "OFF" },
            auto_style,
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("QUEUE: ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{}", app.ready_tasks.len()),
            Style::default().fg(if app.ready_tasks.is_empty() {
                MUTED
            } else {
                WARN
            }),
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("NOTIFY: ", Style::default().fg(MUTED)),
        Span::styled(
            if app.notifications_enabled { "ON" } else { "OFF" },
            if app.notifications_enabled {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled("TOKENS: ", Style::default().fg(MUTED)),
        Span::styled(
            {
                let (inp, out) = app.session_total_tokens();
                format!("{}↑ {}↓", app::format_tokens(inp), app::format_tokens(out))
            },
            Style::default().fg(INFO),
        ),
        Span::styled("  COST: ", Style::default().fg(MUTED)),
        Span::styled(
            app::format_cost(app.session_total_cost()),
            Style::default().fg(if app.session_total_cost() > 1.0 { WARN } else { ACCENT }),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(PANEL_BG));

    f.render_widget(Paragraph::new(line).block(block), area);
}

// ══════════════════════════════════════════════════════════
//  KEYBINDINGS BAR
// ══════════════════════════════════════════════════════════

fn render_keybindings(f: &mut Frame, area: Rect, app: &App) {
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
                ("1-4", "view"),
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
                ("t", "timeout"),
                ("f", "sort"),
                ("F", "filter"),
                ("Tab", "focus"),
                ("j/k", "nav"),
                ("Enter", "detail"),
                ("x", "dismiss"),
                ("X", "dismiss all"),
                ("+/-", "slots"),
                ("1-4", "view"),
                ("?", "help"),
                ("q", "quit"),
            ]
        },
        View::AgentDetail => if app.interactive_mode {
            vec![
                ("Ctrl+]", "detach"),
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
                ("k", "kill"),
                ("?", "help"),
                ("q", "back"),
            ]);
            keys
        },
        View::EventLog => vec![
            ("↑↓", "scroll"),
            ("1-4", "view"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::History => vec![
            ("↑↓", "scroll"),
            ("PgUp/Dn", "page"),
            ("1-4", "view"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::SplitPane => vec![
            ("Tab", "focus pane"),
            ("↑↓", "scroll"),
            ("Enter", "detail"),
            ("g", "pin/unpin"),
            ("1-5", "view"),
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
                        .fg(PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", action), Style::default().fg(MUTED)),
            ]
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(DARK_BG));
    f.render_widget(paragraph, area);
}

// ══════════════════════════════════════════════════════════
//  HELP OVERLAY
// ══════════════════════════════════════════════════════════

fn render_help_overlay(f: &mut Frame, area: Rect) {
    // Center a popup of fixed size
    let popup_width = 64u16.min(area.width.saturating_sub(4));
    let popup_height = 30u16.min(area.height.saturating_sub(4));
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
        .border_style(Style::default().fg(PRIMARY))
        .title(Span::styled(
            " ◈ KEYBOARD SHORTCUTS  [? / Esc to close] ",
            Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL_BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Build lines for all sections
    let mut lines: Vec<Line> = Vec::new();

    fn section_header(title: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("  ── {} ──", title),
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
        ])
    }

    fn key_line(key: &'static str, desc: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {:12}", key), Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled(desc, Style::default().fg(BRIGHT)),
        ])
    }

    // ── Dashboard ──
    lines.push(section_header("DASHBOARD"));
    lines.push(key_line("s", "Spawn agent on selected task"));
    lines.push(key_line("p", "Trigger manual poll / scan"));
    lines.push(key_line("r", "Cycle runtime (Claude/Codex/Copilot)"));
    lines.push(key_line("m", "Cycle model for current runtime"));
    lines.push(key_line("a", "Toggle auto-spawn mode"));
    lines.push(key_line("n", "Toggle desktop notifications on/off"));
    lines.push(key_line("t", "Cycle agent timeout (5m / 15m / 30m / 1h / off)"));
    lines.push(key_line("c", "Scan and clean up orphaned worktrees"));
    lines.push(key_line("f", "Cycle sort mode (priority/type/age/name)"));
    lines.push(key_line("F", "Cycle type filter (bug/feature/task/chore/epic)"));
    lines.push(key_line("Tab", "Toggle focus: Ready Queue ↔ Agents"));
    lines.push(key_line("↑↓ / j/k", "Navigate list  (detail panel updates)"));
    lines.push(key_line("Enter", "Open Agent Detail for selected"));
    lines.push(key_line("f", "Cycle agent status filter: All→Running→Failed→Done→Init (focus: Agents)"));
    lines.push(key_line("x", "Dismiss selected finished agent (focus: Agents)"));
    lines.push(key_line("X", "Dismiss ALL finished agents (focus: Agents)"));
    lines.push(key_line("+/-", "Increase/decrease max concurrent slots"));
    lines.push(key_line("1-4", "Switch view"));
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
    lines.push(key_line("k", "Kill (SIGTERM) current agent + clean up worktree"));
    lines.push(key_line("Esc / q", "Return to Dashboard"));
    lines.push(Line::from(""));

    // ── Agent Detail — Interactive ──
    lines.push(section_header("AGENT DETAIL  (Interactive mode)"));
    lines.push(key_line("Ctrl+]", "Detach from PTY (return to Observe)"));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("All other keys are forwarded to the agent's PTY.", Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(""));

    // ── Event Log ──
    lines.push(section_header("EVENT LOG"));
    lines.push(key_line("↑↓", "Scroll log"));
    lines.push(key_line("1-4", "Switch view"));
    lines.push(key_line("q", "Quit"));
    lines.push(Line::from(""));

    // ── History ──
    lines.push(section_header("HISTORY"));
    lines.push(key_line("↑↓", "Scroll session list"));
    lines.push(key_line("PgUp/PgDn", "Scroll by 10 sessions"));
    lines.push(key_line("1-4", "Switch view"));
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
    // ── Global ──
    lines.push(section_header("GLOBAL"));
    lines.push(key_line("?", "Toggle this help overlay"));
    lines.push(key_line("Ctrl+C", "Force quit"));

    let visible = inner.height as usize;
    let display: Vec<Line> = lines.into_iter().take(visible).collect();

    f.render_widget(Paragraph::new(display).style(Style::default().bg(PANEL_BG)), inner);
}

// ══════════════════════════════════════════════════════════
//  POLL ERROR BANNER
// ══════════════════════════════════════════════════════════

fn render_poll_error_banner(f: &mut Frame, area: Rect, app: &App) {
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
                .bg(DANGER)
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
                .fg(WARN)
                .bg(Color::Rgb(40, 15, 0))
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(
            Paragraph::new(urgent_line).style(Style::default().bg(Color::Rgb(40, 15, 0))),
            second_row,
        );
    }
}

// ══════════════════════════════════════════════════════════
//  SEARCH BAR
// ══════════════════════════════════════════════════════════

fn render_search_bar(f: &mut Frame, area: Rect, app: &App) {
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
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.search_query.as_str(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(WARN)),
        Span::styled(
            match_info.as_str(),
            Style::default().fg(
                if app.search_matches.is_empty() && !app.search_query.is_empty() {
                    DANGER
                } else {
                    MUTED
                },
            ),
        ),
        Span::styled("  [Esc] close", Style::default().fg(MUTED)),
    ]);

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Rgb(15, 15, 30))),
        area,
    );
}

// ══════════════════════════════════════════════════════════
//  VT100 TERMINAL RENDERER
// ══════════════════════════════════════════════════════════

/// Render a vt100 screen directly into the ratatui buffer.
/// Handles colors, bold/italic/underline, cursor position, and search highlights.
fn render_vt100_screen(f: &mut Frame, screen: &vt100::Screen, area: Rect, app: &App) {
    let buf = f.buffer_mut();
    let rows = area.height as usize;
    let cols = area.width as usize;
    let (screen_rows, screen_cols) = (screen.size().0 as usize, screen.size().1 as usize);

    let query_char_len = app.search_query.chars().count();
    let search_active = app.search_active && query_char_len > 0;

    for row in 0..rows.min(screen_rows) {
        // Pre-compute match ranges for this row to avoid per-cell O(n) scan
        let row_matches: Vec<(usize, usize)> = if search_active {
            app.search_matches
                .iter()
                .filter(|&&(mr, _)| mr == row)
                .map(|&(_, mc)| (mc, mc + query_char_len))
                .collect()
        } else {
            Vec::new()
        };
        let current_range: Option<(usize, usize)> = if search_active {
            app.search_matches
                .get(app.search_current_idx)
                .filter(|&&(mr, _)| mr == row)
                .map(|&(_, mc)| (mc, mc + query_char_len))
        } else {
            None
        };

        for col in 0..cols.min(screen_cols) {
            let cell = screen.cell(row as u16, col as u16);
            let x = area.x + col as u16;
            let y = area.y + row as u16;

            if x >= area.x + area.width || y >= area.y + area.height {
                continue;
            }

            if let Some(cell) = cell {
                let is_current = current_range
                    .map(|(s, e)| col >= s && col < e)
                    .unwrap_or(false);
                let is_other_match =
                    !is_current && row_matches.iter().any(|&(s, e)| col >= s && col < e);

                let fg = if is_current {
                    Color::Black
                } else {
                    vt100_color_to_ratatui(cell.fgcolor())
                };
                let bg = if is_current {
                    Color::Rgb(255, 200, 0) // bright yellow — current match
                } else if is_other_match {
                    Color::Rgb(80, 70, 0) // dark amber — other matches
                } else {
                    vt100_color_to_ratatui(cell.bgcolor())
                };

                let mut style = Style::default().fg(fg).bg(bg);

                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() && !is_current && !is_other_match {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                let contents = cell.contents();
                let ch = if contents.is_empty() {
                    ' '
                } else {
                    contents.chars().next().unwrap_or(' ')
                };

                buf[(x, y)].set_char(ch).set_style(style);
            }
        }
    }
}

/// Map vt100 color to ratatui Color.
fn vt100_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Compute the inner (rows, cols) of the terminal output panel given the full
/// terminal size. This mirrors the layout chain: main → agent_detail → output_block.
pub fn compute_pty_area(term_cols: u16, term_rows: u16) -> (u16, u16) {
    // Main vertical layout (from render()):
    //   Length(3)  title bar
    //   Length(1)  tab bar
    //   Min(10)    main content  ← this is agent_detail area
    //   Length(3)  status gauges
    //   Length(3)  info bar
    //   Length(1)  keybindings
    // Chrome = 3+1+3+3+1 = 11
    let content_height = term_rows.saturating_sub(11);

    // Agent detail vertical layout (from render_agent_detail()):
    //   Length(3)  agent header
    //   Min(5)    output area
    let output_height = content_height.saturating_sub(3);

    // Output horizontal split:
    //   Min(40)      output panel
    //   Length(28)   stats panel
    let output_width = term_cols.saturating_sub(28);

    // The output block has Borders::ALL → subtract 2 from each dimension for inner area
    let inner_rows = output_height.saturating_sub(2);
    let inner_cols = output_width.saturating_sub(2);

    (inner_rows, inner_cols)
}

// ══════════════════════════════════════════════════════════
//  HELPERS
// ══════════════════════════════════════════════════════════

fn log_category_color(cat: LogCategory) -> Color {
    match cat {
        LogCategory::System => BRIGHT,
        LogCategory::Incoming => INFO,
        LogCategory::Deploy => ACCENT,
        LogCategory::Complete => ACCENT,
        LogCategory::Alert => DANGER,
        LogCategory::Poll => SECONDARY,
        LogCategory::Timeout => WARN,
    }
}

fn phase_color(phase: AgentPhase) -> Color {
    match phase {
        AgentPhase::Detecting | AgentPhase::Claiming | AgentPhase::Worktree => INFO,
        AgentPhase::Implementing => WARN,
        AgentPhase::Verifying | AgentPhase::Merging | AgentPhase::Closing | AgentPhase::Done => {
            ACCENT
        }
    }
}

/// Render a compact phase step indicator: P0·P1·P2·[P3]·P4·P5·P6·P7
fn render_phase_indicator(phase: AgentPhase) -> Vec<Span<'static>> {
    let all = [
        AgentPhase::Detecting,
        AgentPhase::Claiming,
        AgentPhase::Worktree,
        AgentPhase::Implementing,
        AgentPhase::Verifying,
        AgentPhase::Merging,
        AgentPhase::Closing,
        AgentPhase::Done,
    ];
    let mut spans = Vec::new();
    for (i, p) in all.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("·", Style::default().fg(MUTED)));
        }
        if *p == phase {
            spans.push(Span::styled(
                format!("[{}]", p.short()),
                Style::default()
                    .fg(phase_color(phase))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if *p < phase {
            spans.push(Span::styled(
                p.short().to_string(),
                Style::default().fg(MUTED),
            ));
        } else {
            spans.push(Span::styled(
                p.short().to_string(),
                Style::default().fg(Color::Rgb(40, 40, 55)),
            ));
        }
    }
    spans
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}
