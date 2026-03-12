use crate::app::App;
use crate::types::*;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Tabs,
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
    }

    render_status_gauges(f, chunks[3], app);
    render_info_bar(f, chunks[4], app);
    render_keybindings(f, chunks[5], app);

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
    ];

    let selected = match app.active_view {
        View::Dashboard => 0,
        View::AgentDetail => 1,
        View::EventLog => 2,
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

    render_ready_queue(f, h_chunks[0], app);
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

fn render_agent_panel(f: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focus == Focus::AgentList && app.active_view == View::Dashboard;
    let border_color = if is_focused { ACCENT } else { MUTED };

    let active = app.active_agent_count();
    let title = format!("◆ ACTIVE AGENTS [{}/{}]", active, app.max_concurrent);

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

    let items: Vec<ListItem> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
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
    let header_line = Line::from(vec![
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
    ]);

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

    // Split output area: output on left, stats on right
    let output_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(28)])
        .split(chunks[1]);

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
        render_vt100_screen(f, pty_state.parser.screen(), inner);
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

    // Stats panel
    render_agent_stats(f, output_chunks[1], agent, app);
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

    let lines = vec![
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
        Line::from(""),
        Line::from(vec![
            Span::styled(" ELAPSED ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {}", App::format_elapsed(agent.elapsed_secs)),
                Style::default().fg(BRIGHT),
            ),
        ]),
        Line::from(vec![
            Span::styled(" LINES   ", Style::default().fg(MUTED)),
            Span::styled(format!(" {}", total_lines), Style::default().fg(BRIGHT)),
        ]),
        Line::from(vec![
            Span::styled(" RATE    ", Style::default().fg(MUTED)),
            Span::styled(format!(" {:.1}/s", lines_per_sec), Style::default().fg(ACCENT)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            hex_row,
            Style::default().fg(Color::Rgb(30, 30, 50)),
        )),
    ];

    f.render_widget(Paragraph::new(lines).block(block), area);
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

    let poll_color = if poll_ratio > 0.5 {
        INFO
    } else if poll_ratio > 0.2 {
        WARN
    } else {
        DANGER
    };

    let poll_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(poll_color))
                .title(Span::styled(
                    " POLL CYCLE ",
                    Style::default()
                        .fg(poll_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .style(Style::default().bg(PANEL_BG)),
        )
        .gauge_style(Style::default().fg(poll_color).bg(Color::Rgb(20, 20, 30)))
        .ratio(poll_ratio)
        .label(Span::styled(
            format!("{:.0}s", app.poll_countdown),
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
        View::Dashboard => vec![
            ("s", "spawn"),
            ("p", "poll"),
            ("r", "runtime"),
            ("m", "model"),
            ("a", "auto"),
            ("f", "sort"),
            ("F", "filter"),
            ("Tab", "focus"),
            ("j/k", "nav"),
            ("Enter", "detail"),
            ("+/-", "slots"),
            ("1-3", "view"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::AgentDetail => if app.interactive_mode {
            vec![
                ("Ctrl+]", "detach"),
                ("", "— all other keys forwarded to agent PTY —"),
            ]
        } else {
            vec![
                ("i", "interact"),
                ("↑↓", "scroll"),
                ("PgUp/Dn", "page"),
                ("Home/End", "top/bottom"),
                ("←→", "prev/next"),
                ("Esc", "back"),
                ("k", "kill"),
                ("?", "help"),
                ("q", "back"),
            ]
        },
        View::EventLog => vec![
            ("↑↓", "scroll"),
            ("1-3", "view"),
            ("?", "help"),
            ("q", "quit"),
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
    lines.push(key_line("f", "Cycle sort mode (priority/type/age/name)"));
    lines.push(key_line("F", "Cycle type filter (bug/feature/task/chore/epic)"));
    lines.push(key_line("Tab", "Toggle focus: Ready Queue ↔ Agents"));
    lines.push(key_line("↑↓ / j/k", "Navigate list"));
    lines.push(key_line("Enter", "Open Agent Detail for selected"));
    lines.push(key_line("+/-", "Increase/decrease max concurrent slots"));
    lines.push(key_line("1-3", "Switch view"));
    lines.push(key_line("q", "Quit"));
    lines.push(Line::from(""));

    // ── Agent Detail — Observe ──
    lines.push(section_header("AGENT DETAIL  (Observe mode)"));
    lines.push(key_line("i", "Attach interactive PTY session"));
    lines.push(key_line("↑↓", "Scroll output one line"));
    lines.push(key_line("PgUp/PgDn", "Scroll output by page"));
    lines.push(key_line("Home/End", "Jump to top / re-engage auto-follow"));
    lines.push(key_line("←/→", "Previous / next agent"));
    lines.push(key_line("k", "Kill (SIGTERM) current agent"));
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
    lines.push(key_line("1-3", "Switch view"));
    lines.push(key_line("q", "Quit"));
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
//  VT100 TERMINAL RENDERER
// ══════════════════════════════════════════════════════════

/// Render a vt100 screen directly into the ratatui buffer.
/// Handles colors, bold/italic/underline, and cursor position.
fn render_vt100_screen(f: &mut Frame, screen: &vt100::Screen, area: Rect) {
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
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}
