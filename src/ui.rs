use crate::app::App;
use crate::theme::Theme;
use crate::types::*;
use chrono::Utc;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Tabs, Wrap,
    },
    Frame,
};

// ══════════════════════════════════════════════════════════
//  THEMED BLOCK HELPER
// ══════════════════════════════════════════════════════════

fn primary_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.muted))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(theme.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.panel_bg))
}

// ══════════════════════════════════════════════════════════
//  MAIN RENDER
// ══════════════════════════════════════════════════════════

pub fn render(f: &mut Frame, app: &mut App) {
    let dark_bg = app.theme.dark_bg;
    let area = f.area();
    let compact_rows = area.height < 40;

    // Clear all cells first to prevent artifacts when switching views
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(dark_bg)),
        area,
    );

    if compact_rows {
        // Compact vertical layout: drop status gauges, shrink info bar to 1 line
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title bar
                Constraint::Length(1), // Tab bar
                Constraint::Length(1), // Status summary
                Constraint::Min(10),  // Main content
                Constraint::Length(1), // Info bar (compact)
                Constraint::Length(1), // Keybindings
            ])
            .split(area);

        render_title_bar(f, chunks[0], app);
        render_tab_bar(f, chunks[1], app);
        render_status_summary(f, chunks[2], app);

        match app.active_view {
            View::Dashboard => render_dashboard(f, chunks[3], app),
            View::AgentDetail => render_agent_detail(f, chunks[3], app),
            View::EventLog => render_event_log(f, chunks[3], app),
            View::History => render_history(f, chunks[3], app),
            View::SplitPane => render_split_pane(f, chunks[3], app),
            View::WorktreeOverview => render_worktree_overview(f, chunks[3], app),
            View::DepGraph => render_dep_graph(f, chunks[3], app),
        }

        render_info_bar_compact(f, chunks[4], app);
        render_keybindings(f, chunks[5], app);

        if !app.last_poll_ok {
            render_poll_error_banner(f, chunks[3], app);
        }

        if app.jump_active {
            render_jump_bar(f, chunks[5], app);
        }
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title bar
                Constraint::Length(1), // Tab bar
                Constraint::Length(1), // Status summary
                Constraint::Min(10),  // Main content
                Constraint::Length(3), // Status gauges
                Constraint::Length(3), // Info bar
                Constraint::Length(1), // Keybindings
            ])
            .split(area);

        render_title_bar(f, chunks[0], app);
        render_tab_bar(f, chunks[1], app);
        render_status_summary(f, chunks[2], app);

        match app.active_view {
            View::Dashboard => render_dashboard(f, chunks[3], app),
            View::AgentDetail => render_agent_detail(f, chunks[3], app),
            View::EventLog => render_event_log(f, chunks[3], app),
            View::History => render_history(f, chunks[3], app),
            View::SplitPane => render_split_pane(f, chunks[3], app),
            View::WorktreeOverview => render_worktree_overview(f, chunks[3], app),
            View::DepGraph => render_dep_graph(f, chunks[3], app),
        }

        render_status_gauges(f, chunks[4], app);
        render_info_bar(f, chunks[5], app);
        render_keybindings(f, chunks[6], app);

        if !app.last_poll_ok {
            render_poll_error_banner(f, chunks[3], app);
        }

        if app.jump_active {
            render_jump_bar(f, chunks[6], app);
        }
    }

    if app.show_help {
        render_help_overlay(f, area, &app.theme);
    }

    if let Some(agent_id) = app.confirm_complete_agent_id {
        render_complete_confirm_dialog(f, area, app, agent_id);
    }

    if let Some(agent_id) = app.confirm_kill_agent_id {
        render_kill_confirm_dialog(f, area, app, agent_id);
    }

    if app.confirm_quit {
        render_quit_confirm_dialog(f, area, app);
    }

    if app.issue_creation_active {
        render_issue_creation_form(f, area, app);
    }
}

// ══════════════════════════════════════════════════════════
//  TITLE BAR
// ══════════════════════════════════════════════════════════

fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  TAB BAR
// ══════════════════════════════════════════════════════════

fn render_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    // Compute badge counts
    let ready_count = app.ready_tasks.len();
    let active_count = app.agents.iter().filter(|a| {
        a.status == AgentStatus::Running || a.status == AgentStatus::Starting
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

// ══════════════════════════════════════════════════════════
//  STATUS SUMMARY BAR
// ══════════════════════════════════════════════════════════

fn render_status_summary(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  DASHBOARD VIEW
// ══════════════════════════════════════════════════════════

fn render_dashboard(f: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;
    let term = f.area();
    let compact_rows = term.height < 40;
    let compact_cols = term.width < 100;

    // When compact: hide charts and give full width to event log
    let show_bottom = !compact_rows || area.height > 12;
    let v_chunks = if show_bottom && !compact_cols {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),     // Top: ready queue + agents
                Constraint::Length(10), // Bottom: throughput + completions + mini log
            ])
            .split(area)
    } else if show_bottom && compact_cols {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),    // Top: ready queue + agents
                Constraint::Length(3), // Bottom: single-line event log
            ])
            .split(area)
    } else {
        // Very compact: no bottom panel at all
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8)])
            .split(area)
    };

    // Alert banner overlay
    if let Some((ref msg, _)) = app.alert_message {
        let blink = (app.frame_count / 3).is_multiple_of(2);
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
                    .bg(t.danger)
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

    // Left column: queue list on top, blocked panel, task detail preview on bottom
    if compact_rows {
        if app.blocked_tasks.is_empty() {
            app.layout_areas.ready_queue = Some(h_chunks[0]);
            app.layout_areas.blocked_queue = None;
            render_ready_queue(f, h_chunks[0], app);
        } else {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(6)])
                .split(h_chunks[0]);
            app.layout_areas.ready_queue = Some(left_chunks[0]);
            render_ready_queue(f, left_chunks[0], app);
            app.layout_areas.blocked_queue = Some(left_chunks[1]);
            render_blocked_queue(f, left_chunks[1], app);
        }
    } else {
        if app.blocked_tasks.is_empty() {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(9)])
                .split(h_chunks[0]);
            app.layout_areas.ready_queue = Some(left_chunks[0]);
            app.layout_areas.blocked_queue = None;
            render_ready_queue(f, left_chunks[0], app);
            render_task_preview(f, left_chunks[1], app);
        } else {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(4),
                    Constraint::Length(6),
                    Constraint::Length(9),
                ])
                .split(h_chunks[0]);
            app.layout_areas.ready_queue = Some(left_chunks[0]);
            render_ready_queue(f, left_chunks[0], app);
            app.layout_areas.blocked_queue = Some(left_chunks[1]);
            render_blocked_queue(f, left_chunks[1], app);
            render_task_preview(f, left_chunks[2], app);
        }
    }
    render_agent_panel(f, h_chunks[1], app);

    // Bottom panels
    if v_chunks.len() > 1 {
        if compact_cols {
            // < 100 cols: hide charts, full-width single-line event log
            render_mini_event_log(f, v_chunks[1], app);
        } else {
            let bottom_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(50),
                    Constraint::Percentage(30),
                ])
                .split(v_chunks[1]);
            render_throughput_chart(f, bottom_chunks[0], app);
            render_completions_feed(f, bottom_chunks[1], app);
            render_mini_event_log(f, bottom_chunks[2], app);
        }
    }
}

fn render_throughput_chart(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let data: Vec<u64> = app
        .throughput_history
        .iter()
        .map(|&v| v as u64)
        .collect();

    let max_val = data.iter().copied().max().unwrap_or(1).max(1);
    let current = data.last().copied().unwrap_or(0);
    let label = format!("now: {}/s  peak: {}/s", current, max_val);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " THROUGHPUT ",
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            format!(" {} ", label),
            Style::default().fg(t.muted),
        )))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chart_height = inner.height as u64;
    // Block elements: space + 8 fractional blocks
    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // Take only the last N data points that fit the width
    let visible_count = inner.width as usize;
    let skip = data.len().saturating_sub(visible_count);
    let visible_data: Vec<u64> = data.iter().skip(skip).copied().collect();

    // Build rows from top to bottom
    let mut lines: Vec<Line> = Vec::with_capacity(chart_height as usize);
    for row in 0..chart_height {
        let row_from_bottom = chart_height - 1 - row;
        let mut spans: Vec<Span> = Vec::with_capacity(visible_count);

        // Pad left if data is shorter than width
        let pad = visible_count.saturating_sub(visible_data.len());
        if pad > 0 {
            spans.push(Span::styled(
                " ".repeat(pad),
                Style::default().bg(t.panel_bg),
            ));
        }

        for &val in &visible_data {
            // How many sub-levels (out of chart_height * 8) does this value fill?
            let filled_sub = val * chart_height * 8 / max_val;
            // Sub-level position for the bottom of this row
            let row_base = row_from_bottom * 8;
            let level = if filled_sub >= row_base + 8 {
                8 // Full block
            } else if filled_sub > row_base {
                (filled_sub - row_base) as usize
            } else {
                0 // Empty
            };

            // Color gradient based on how high this bar reaches relative to max
            let ratio = val as f64 / max_val as f64;
            let fg = if ratio > 0.75 {
                t.accent
            } else if ratio > 0.4 {
                t.primary
            } else if ratio > 0.0 {
                t.dim_accent
            } else {
                t.panel_bg
            };

            spans.push(Span::styled(
                BLOCKS[level].to_string(),
                Style::default().fg(fg).bg(t.panel_bg),
            ));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), inner);
}


fn render_completions_feed(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let count = app.recent_completions.len();
    let title = format!(" RECENT COMPLETIONS ({}) ", count);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            title,
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(area);
    let visible = inner.height as usize;

    if app.recent_completions.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No completions yet",
            Style::default().fg(t.muted),
        )))
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    // Show most recent entries (auto-scroll: newest at bottom)
    let items: Vec<ListItem> = app
        .recent_completions
        .iter()
        .rev()
        .take(visible)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|rec| {
            let status_sym = if rec.success { "✓" } else { "✗" };
            let status_color = if rec.success { t.accent } else { t.danger };

            // Format duration
            let duration = if rec.elapsed_secs >= 3600 {
                format!("{}h{:02}m", rec.elapsed_secs / 3600, (rec.elapsed_secs % 3600) / 60)
            } else if rec.elapsed_secs >= 60 {
                format!("{}m{:02}s", rec.elapsed_secs / 60, rec.elapsed_secs % 60)
            } else {
                format!("{}s", rec.elapsed_secs)
            };

            // Truncate title to fit
            let max_title = 20;
            let title_display = if rec.title.len() > max_title {
                format!("{}\u{2026}", &rec.title[..max_title - 1])
            } else {
                rec.title.clone()
            };

            // Short model name (last component)
            let model_short = rec.model.split('-').next_back().unwrap_or(&rec.model);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", status_sym),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} ", rec.task_id),
                    Style::default().fg(t.bright),
                ),
                Span::styled(
                    format!("{} ", title_display),
                    Style::default().fg(t.muted),
                ),
                Span::styled(
                    format!("[{}] ", rec.runtime),
                    Style::default().fg(t.info),
                ),
                Span::styled(
                    format!("{} ", model_short),
                    Style::default().fg(t.muted),
                ),
                Span::styled(
                    duration,
                    Style::default().fg(t.secondary),
                ),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(block), area);
}

fn render_mini_event_log(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " RECENT EVENTS ",
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(area);
    let visible = inner.height as usize;

    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .take(visible)
        .map(|entry| {
            let cat_color = log_category_color(entry.category, t);

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", entry.timestamp), Style::default().fg(t.muted)),
                Span::styled(
                    format!("[{}] ", entry.category.label()),
                    Style::default().fg(cat_color),
                ),
                Span::styled(
                    truncate_str(&entry.message, 40),
                    Style::default().fg(t.bright),
                ),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(block), area);
}

/// Returns (age_label, age_color) based on how old the issue is.
/// Color bands: <1d neutral, 1-3d yellow, 3-7d orange, 7d+ red.
fn age_badge(created_at: Option<&str>, t: &Theme) -> (String, Color) {
    let Some(ts) = created_at else {
        return (String::new(), t.muted);
    };

    let Ok(created) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return (String::new(), t.muted);
    };

    let age = Utc::now().signed_duration_since(created);
    let days = age.num_days();
    let hours = age.num_hours();

    let label = if days >= 1 {
        format!("{}d", days)
    } else {
        format!("{}h", hours.max(0))
    };

    let color = if days >= 7 {
        t.danger // red
    } else if days >= 3 {
        t.primary // orange
    } else if days >= 1 {
        t.warn // yellow
    } else {
        t.muted // neutral
    };

    (label, color)
}

fn render_ready_queue(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let is_focused = app.focus == Focus::ReadyQueue && app.active_view == View::Dashboard;
    let border_color = if is_focused { t.accent } else { t.muted };

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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title.trim_end()),
            Style::default()
                .fg(if is_focused { t.bright } else { t.muted })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    if filtered.is_empty() {
        let empty_msg = if app.ready_tasks.is_empty() {
            Line::from(vec![
                Span::styled("  No ready tasks — ", Style::default().fg(t.muted)),
                Span::styled("STANDBY", Style::default().fg(t.warn)),
            ])
        } else {
            Line::from(vec![
                Span::styled("  No tasks match filter — ", Style::default().fg(t.muted)),
                Span::styled("press F to change", Style::default().fg(t.warn)),
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
                0 => Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
                1 => Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
                2 => Style::default().fg(t.bright),
                3 => Style::default().fg(t.muted),
                _ => Style::default().fg(t.muted),
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
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            let (age_label, age_color) = age_badge(task.created_at.as_deref(), t);

            let mut spans = vec![
                sel_indicator,
                Span::styled(format!("P{} ", priority), p_style),
                Span::styled(format!("[{}] ", type_str), Style::default().fg(t.muted)),
            ];
            if !age_label.is_empty() {
                spans.push(Span::styled(
                    format!("{} ", age_label),
                    Style::default().fg(age_color).add_modifier(Modifier::BOLD),
                ));
            }
            spans.push(Span::styled(format!("{}: ", task.id), Style::default().fg(t.secondary)));
            spans.push(Span::styled(truncate_str(&task.title, 30), Style::default().fg(t.bright)));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 35))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut app.task_list_state.clone());
}

fn render_blocked_queue(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let is_focused = app.focus == Focus::BlockedQueue && app.active_view == View::Dashboard;
    let border_color = if is_focused { t.accent } else { t.muted };

    let count = app.blocked_tasks.len();
    let title = format!("BLOCKED [{}]", count);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(if is_focused { t.danger } else { t.muted })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    if app.blocked_tasks.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  No blocked issues",
            Style::default().fg(t.muted),
        )))
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .blocked_tasks
        .iter()
        .enumerate()
        .map(|(i, bt)| {
            let task = &bt.task;
            let priority = task.priority.unwrap_or(3);
            let p_style = match priority {
                0 => Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
                1 => Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
                2 => Style::default().fg(t.bright),
                _ => Style::default().fg(t.muted),
            };

            let sel_indicator = if Some(i) == app.blocked_list_state.selected() && is_focused {
                Span::styled(
                    " \u{25b8} ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("   ", Style::default())
            };

            let dep_badge = format!("({} dep{}) ", bt.remaining_deps, if bt.remaining_deps == 1 { "" } else { "s" });

            let spans = vec![
                sel_indicator,
                Span::styled(format!("P{} ", priority), p_style),
                Span::styled(
                    dep_badge,
                    Style::default().fg(t.danger),
                ),
                Span::styled(format!("{}: ", task.id), Style::default().fg(t.secondary)),
                Span::styled(truncate_str(&task.title, 26), Style::default().fg(t.muted)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 35))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut app.blocked_list_state.clone());
}

fn render_task_preview(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let is_focused = app.focus == Focus::ReadyQueue && app.active_view == View::Dashboard;
    let border_color = if is_focused { t.primary } else { t.muted };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " DETAIL ",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let task = match app.selected_task() {
        Some(t) => t,
        None => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    " Select a task to view details",
                    Style::default().fg(t.muted),
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
        0 => t.danger,
        1 => t.bright,
        2 => t.bright,
        _ => t.muted,
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
        Span::styled(format!("[{}] ", type_str), Style::default().fg(t.muted)),
        Span::styled(format!("P{}  ", priority), Style::default().fg(p_color)),
    ];
    if !assignee.is_empty() {
        meta_spans.push(Span::styled(assignee, Style::default().fg(t.muted)));
    }
    if !labels_str.is_empty() {
        meta_spans.push(Span::styled(
            format!("  ◦ {}", labels_str),
            Style::default().fg(t.muted),
        ));
    }

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            task.title.as_str(),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        )),
        Line::from(meta_spans),
        Line::from(""),
    ];
    lines.extend(render_markdown(description, t));

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );
}

// ── Lightweight markdown → ratatui Lines ────────────────────────────
// Handles: # headers, **bold**, *italic*, `inline code`, ```code blocks```,
//          - / * / + list items, - [ ] / - [x] checkboxes.
// Falls back to raw text on any parse failure.

fn render_markdown(text: &str, t: &Theme) -> Vec<Line<'static>> {
    fn parse_inline(text: &str, base_style: Style, code_color: Color) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut remaining = text;

        while !remaining.is_empty() {
            // Find next markdown marker
            let next_marker = [
                remaining.find("**"),
                remaining.find("*").filter(|&i| {
                    // Only match single * that isn't part of **
                    !remaining[i..].starts_with("**")
                }),
                remaining.find('`'),
            ];

            let earliest = next_marker.iter().filter_map(|x| *x).min();

            match earliest {
                None => {
                    spans.push(Span::styled(remaining.to_string(), base_style));
                    break;
                }
                Some(pos) => {
                    // Push text before marker
                    if pos > 0 {
                        spans.push(Span::styled(remaining[..pos].to_string(), base_style));
                    }
                    let after = &remaining[pos..];

                    if let Some(stripped) = after.strip_prefix("**") {
                        // Bold
                        if let Some(end) = stripped.find("**") {
                            let content = &stripped[..end];
                            spans.push(Span::styled(
                                content.to_string(),
                                base_style.add_modifier(Modifier::BOLD),
                            ));
                            remaining = &stripped[end + 2..];
                        } else {
                            spans.push(Span::styled(after.to_string(), base_style));
                            break;
                        }
                    } else if let Some(stripped) = after.strip_prefix('`') {
                        // Inline code
                        if let Some(end) = stripped.find('`') {
                            let content = &stripped[..end];
                            spans.push(Span::styled(
                                content.to_string(),
                                Style::default().fg(code_color),
                            ));
                            remaining = &stripped[end + 1..];
                        } else {
                            spans.push(Span::styled(after.to_string(), base_style));
                            break;
                        }
                    } else if let Some(stripped) = after.strip_prefix('*') {
                        // Italic
                        if let Some(end) = stripped.find('*') {
                            let content = &stripped[..end];
                            spans.push(Span::styled(
                                content.to_string(),
                                base_style.add_modifier(Modifier::ITALIC),
                            ));
                            remaining = &stripped[end + 1..];
                        } else {
                            spans.push(Span::styled(after.to_string(), base_style));
                            break;
                        }
                    } else {
                        spans.push(Span::styled(after.to_string(), base_style));
                        break;
                    }
                }
            }
        }
        spans
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let base = Style::default().fg(t.muted);

    for line in text.lines() {
        // Fenced code blocks
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::Rgb(140, 140, 160)).add_modifier(Modifier::DIM),
            )));
            continue;
        }

        let trimmed = line.trim_start();

        // Headers: # ## ###
        if let Some(rest) = trimmed.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Checkbox list items: - [ ] or - [x]
        if let Some(rest) = trimmed.strip_prefix("- [x] ").or_else(|| trimmed.strip_prefix("- [X] ")) {
            let mut spans = vec![Span::styled(
                "  ✓ ".to_string(),
                Style::default().fg(t.accent),
            )];
            spans.extend(parse_inline(rest, base, t.warn));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            let mut spans = vec![Span::styled(
                "  ○ ".to_string(),
                Style::default().fg(t.muted),
            )];
            spans.extend(parse_inline(rest, base, t.warn));
            lines.push(Line::from(spans));
            continue;
        }

        // List items: - / * / +
        let list_rest = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "));
        if let Some(rest) = list_rest {
            let mut spans = vec![Span::styled(
                "  • ".to_string(),
                Style::default().fg(t.muted),
            )];
            spans.extend(parse_inline(rest, base, t.warn));
            lines.push(Line::from(spans));
            continue;
        }

        // Empty lines
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Normal text with inline formatting
        lines.push(Line::from(parse_inline(trimmed, base, t.warn)));
    }

    lines
}

fn render_agent_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let is_focused = app.focus == Focus::AgentList && app.active_view == View::Dashboard;
    let border_color = if is_focused { t.accent } else { t.muted };

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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(if is_focused { t.bright } else { t.muted })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    if app.agents.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::styled("  No agents deployed — ", Style::default().fg(t.muted)),
            Span::styled("IDLE", Style::default().fg(t.warn)),
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
            Span::styled(msg, Style::default().fg(t.muted)),
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
                AgentStatus::Starting => Style::default().fg(t.warn),
                AgentStatus::Running => Style::default().fg(t.accent),
                AgentStatus::Completed => Style::default().fg(t.info),
                AgentStatus::Failed => Style::default().fg(t.danger),
            };

            let runtime_style = Style::default().fg(t.muted);

            let elapsed = App::format_elapsed(agent.elapsed_secs);

            let sel_indicator = if Some(i) == app.agent_list_state.selected() && is_focused {
                Span::styled(
                    " ▸ ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
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
                        .fg(phase_color(agent.phase, t))
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
                        .fg(t.bright)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", agent.task.id), Style::default().fg(t.muted)),
                Span::styled(format!("[{}] ", agent.runtime.name()), runtime_style),
                Span::styled(status_text.to_string(), status_style),
                phase_badge,
                Span::styled(
                    format!("  ({} lines)", line_count),
                    Style::default().fg(t.muted),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 35))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, area, &mut app.agent_list_state.clone());
}

// ══════════════════════════════════════════════════════════
//  AGENT DETAIL VIEW
// ══════════════════════════════════════════════════════════

const DIAGNOSTICS_PANEL_WIDTH: u16 = 56;
const DIAGNOSTICS_PANEL_THRESHOLD: u16 = 148;

fn render_agent_detail(f: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;
    let agent = app
        .selected_agent_id
        .and_then(|id| app.agents.iter().find(|a| a.id == id));

    let agent = match agent {
        Some(a) => a,
        None => {
            let block = primary_block("NO AGENT SELECTED", t);
            let p = Paragraph::new("Press ESC to return to dashboard")
                .block(block)
                .style(Style::default().fg(t.muted));
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
        AgentStatus::Starting => t.warn,
        AgentStatus::Running => t.accent,
        AgentStatus::Completed => t.info,
        AgentStatus::Failed => t.danger,
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
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  ", Style::default().fg(t.muted)),
        Span::styled(agent.task.id.as_str(), Style::default().fg(t.bright)),
        Span::styled(
            format!("  [{}]", agent.runtime.name()),
            Style::default().fg(t.muted),
        ),
        Span::styled(
            format!("  {}", agent.model),
            Style::default().fg(t.muted),
        ),
        Span::styled("  /  ", Style::default().fg(t.muted)),
        Span::styled(
            status_str,
            Style::default()
                .fg(header_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  ", Style::default().fg(t.muted)),
        Span::styled(
            App::format_elapsed(agent.elapsed_secs),
            Style::default().fg(t.bright),
        ),
    ];
    if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
        header_spans.push(Span::styled("  /  ", Style::default().fg(t.muted)));
        header_spans.extend(render_phase_indicator(agent.phase, t));
    }
    let header_line = Line::from(header_spans);

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " AGENT TELEMETRY ",
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    f.render_widget(
        Paragraph::new(header_line).block(header_block),
        chunks[0],
    );

    // Split output area: output on left, diagnostics/diff on right.
    // Hide diagnostics until the terminal is wide enough to keep the output
    // panel comfortable after widening diagnostics.
    let narrow_cols = f.area().width < DIAGNOSTICS_PANEL_THRESHOLD;
    let output_chunks = if narrow_cols && !app.show_diff_panel {
        // No sidebar — full width for terminal output
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40)])
            .split(chunks[1])
    } else if app.show_diff_panel {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(40),
                Constraint::Length(DIAGNOSTICS_PANEL_WIDTH),
            ])
            .split(chunks[1])
    };

    // Output area — use PseudoTerminal widget if PTY is active, else legacy text view
    let mode_label = if app.interactive_mode { "INTERACTIVE" } else { "OBSERVE" };
    let mode_color = if app.interactive_mode { t.accent } else { t.muted };
    let output_title = format!("TERMINAL [{}]", mode_label);

    let output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.interactive_mode { t.primary } else { t.muted }))
        .title(Span::styled(
            format!(" {} ", output_title),
            Style::default()
                .fg(mode_color)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

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
                    Span::styled(format!("{:4} │ ", line_num), Style::default().fg(t.muted)),
                    Span::styled(line.as_str(), Style::default().fg(t.accent)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(output_block);
        f.render_widget(paragraph, output_chunks[0]);

        if total_lines > visible_height {
            let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::default().fg(t.muted)),
                output_chunks[0],
                &mut scrollbar_state,
            );
        }
    }

    // Right panel: diff or diagnostics (hidden when below the diagnostics breakpoint and no diff panel)
    if output_chunks.len() > 1 {
        if app.show_diff_panel {
            render_diff_panel(f, output_chunks[1], app);
        } else {
            render_agent_stats(f, output_chunks[1], agent, app);
        }
    }
}


// ══════════════════════════════════════════════════════════
//  SPLIT-PANE VIEW
// ══════════════════════════════════════════════════════════

fn render_split_pane(f: &mut Frame, area: Rect, app: &mut App) {
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
                        .map(|line| {
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
    let t = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            " DIAGNOSTICS ",
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let total_lines = app.agent_line_count(agent.id);
    let elapsed = agent.elapsed_secs.max(1);
    let lines_per_sec = total_lines as f64 / elapsed as f64;

    let status_indicator = match agent.status {
        AgentStatus::Starting => ("INITIALIZING", t.muted),
        AgentStatus::Running => ("ACTIVE", t.accent),
        AgentStatus::Completed => ("COMPLETE", t.accent),
        AgentStatus::Failed => ("TERMINATED", t.danger),
    };

    let priority_str = agent
        .task
        .priority
        .map(|p| format!("P{}", p))
        .unwrap_or_else(|| "P?".into());

    let retry_color = if agent.retry_count >= app.max_retries {
        t.danger
    } else if agent.retry_count > 0 {
        t.warn
    } else {
        t.muted
    };

    let worktree_line = agent.worktree_path.as_deref().map(|wt_path| {
        let (wt_label, wt_color) = if agent.worktree_cleaned {
            ("(cleaned)", t.muted)
        } else {
            match agent.status {
                AgentStatus::Starting | AgentStatus::Running => ("(active)", t.accent),
                _ => ("(pending)", t.warn),
            }
        };
        Line::from(vec![
            Span::styled(" WRKTR   ", Style::default().fg(t.muted)),
            Span::styled(format!(" {} ", wt_label), Style::default().fg(wt_color)),
            Span::styled(
                std::path::Path::new(wt_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(wt_path)
                    .to_string(),
                Style::default().fg(t.muted),
            ),
        ])
    });

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" STATUS  ", Style::default().fg(t.muted)),
            Span::styled(
                status_indicator.0,
                Style::default()
                    .fg(status_indicator.1)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" RUNTIME ", Style::default().fg(t.muted)),
            Span::styled(
                agent.runtime.name(),
                Style::default().fg(t.bright),
            ),
        ]),
        Line::from(vec![
            Span::styled(" TASK    ", Style::default().fg(t.muted)),
            Span::styled(&agent.task.id, Style::default().fg(t.bright)),
        ]),
        Line::from(vec![
            Span::styled(" PRIORITY", Style::default().fg(t.muted)),
            Span::styled(format!(" {}", priority_str), Style::default().fg(t.bright)),
        ]),
        Line::from(vec![
            Span::styled(" RETRIES ", Style::default().fg(t.muted)),
            Span::styled(
                format!(" {}/{}", agent.retry_count, app.max_retries),
                Style::default().fg(retry_color),
            ),
        ]),
        Line::from(vec![
            Span::styled(" PHASE   ", Style::default().fg(t.muted)),
            Span::styled(
                format!(" {} {}", agent.phase.short(), agent.phase.label()),
                Style::default()
                    .fg(phase_color(agent.phase, t))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" TMPL    ", Style::default().fg(t.muted)),
            Span::styled(
                format!(" {}", agent.template_name),
                Style::default().fg(t.muted),
            ),
        ]),
    ];
    if let Some(wl) = worktree_line {
        lines.push(wl);
    }
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled(" ELAPSED ", Style::default().fg(t.muted)),
            Span::styled(
                format!(" {}", App::format_elapsed(agent.elapsed_secs)),
                Style::default().fg(t.bright),
            ),
        ]),
        Line::from(vec![
            Span::styled(" LINES   ", Style::default().fg(t.muted)),
            Span::styled(format!(" {}", total_lines), Style::default().fg(t.bright)),
        ]),
        Line::from(vec![
            Span::styled(" RATE    ", Style::default().fg(t.muted)),
            Span::styled(format!(" {:.1}/s", lines_per_sec), Style::default().fg(t.accent)),
        ]),
    ]);

    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ══════════════════════════════════════════════════════════
//  DIFF PANEL
// ══════════════════════════════════════════════════════════

fn render_diff_panel(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
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
        .border_style(Style::default().fg(t.muted))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(t.bright)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg));

    let inner = block.inner(area);
    let visible_height = inner.height as usize;

    let diff_data = match &app.diff_data {
        Some(d) => d,
        None => {
            // No data yet — show a loading indicator
            let loading = Paragraph::new(Line::from(Span::styled(
                "  Fetching diff...",
                Style::default().fg(t.muted),
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
                Style::default().fg(t.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Worktree is clean",
                Style::default().fg(t.dim_accent),
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
            Span::styled("M ", Style::default().fg(t.warn).add_modifier(Modifier::BOLD)),
            Span::styled(file.as_str(), Style::default().fg(t.bright)),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(inner.width.saturating_sub(0) as usize),
        Style::default().fg(t.muted),
    )));

    // Build diff lines with color coding
    for diff_line in &diff_data.lines {
        let (style, prefix) = if diff_line.starts_with('+') && !diff_line.starts_with("+++") {
            (Style::default().fg(t.accent), "")
        } else if diff_line.starts_with('-') && !diff_line.starts_with("---") {
            (Style::default().fg(t.danger), "")
        } else if diff_line.starts_with("@@") {
            (Style::default().fg(t.muted).add_modifier(Modifier::BOLD), "")
        } else if diff_line.starts_with("diff --git") {
            (Style::default().fg(t.bright).add_modifier(Modifier::BOLD), "")
        } else if diff_line.starts_with("index ") || diff_line.starts_with("---") || diff_line.starts_with("+++") {
            (Style::default().fg(t.muted), "")
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
                .style(Style::default().fg(t.muted)),
            area,
            &mut scrollbar_state,
        );
    }
}

// ══════════════════════════════════════════════════════════
//  EVENT LOG VIEW
// ══════════════════════════════════════════════════════════

fn render_event_log(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let total = app.event_log.len();
    let title = match app.log_category_filter {
        None => "◆ SYSTEM LOG".to_string(),
        Some(cat) => {
            let filtered = app.event_log.iter().filter(|e| e.category == cat).count();
            format!("◆ EVENT LOG [{}/{}] [{}]", filtered, total, cat.label())
        }
    };
    let block = primary_block(&title, t);

    if app.event_log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No events recorded",
            Style::default().fg(t.muted),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let inner = block.inner(area);
    let visible = inner.height as usize;

    let filtered_entries: Vec<&LogEntry> = match app.log_category_filter {
        None => app.event_log.iter().collect(),
        Some(cat) => app.event_log.iter().filter(|e| e.category == cat).collect(),
    };

    let items: Vec<ListItem> = filtered_entries
        .iter()
        .skip(app.log_scroll)
        .take(visible)
        .map(|entry| {
            let cat_color = log_category_color(entry.category, t);

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", entry.timestamp), Style::default().fg(t.muted)),
                Span::styled(
                    format!("[{}]", entry.category.label()),
                    Style::default()
                        .fg(cat_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", entry.message), Style::default().fg(t.bright)),
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
    let t = &app.theme;
    let (total_sessions, all_completed, all_failed, avg_duration) = app.aggregate_stats();
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
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:16} ", short_id), Style::default().fg(t.muted)),
                Span::styled(format!("started: {:<22} ", &session.started_at[..session.started_at.len().min(19)]), Style::default().fg(t.muted)),
                Span::styled(format!("done:{:>4} ", session.total_completed), Style::default().fg(t.accent)),
                Span::styled(format!("fail:{:>3} ", session.total_failed), Style::default().fg(if session.total_failed > 0 { t.danger } else { t.muted })),
                Span::styled(format!("{:>5.1}%", rate), Style::default().fg(rate_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {:>3} agents", session.agents.len()), Style::default().fg(t.muted)),
            ]))
        })
        .collect();

    f.render_widget(List::new(items).block(sessions_block), v_chunks[1]);
}

// ══════════════════════════════════════════════════════════
//  DEPENDENCY GRAPH
// ══════════════════════════════════════════════════════════

fn dep_status_color(status: &str, t: &Theme) -> Color {
    match status {
        "closed" => t.accent,
        "in_progress" => t.info,
        "blocked" => t.danger,
        "deferred" => t.warn,
        _ => t.muted, // "open" and others
    }
}

fn dep_status_symbol(status: &str) -> &'static str {
    match status {
        "closed" => "✓",
        "in_progress" => "▶",
        "blocked" => "✗",
        "deferred" => "◌",
        _ => "○", // "open"
    }
}

fn render_dep_graph(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  WORKTREE OVERVIEW
// ══════════════════════════════════════════════════════════

fn render_worktree_overview(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  STATUS GAUGES
// ══════════════════════════════════════════════════════════

fn render_status_gauges(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  t.info BAR
// ══════════════════════════════════════════════════════════

fn render_info_bar(f: &mut Frame, area: Rect, app: &App) {
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
fn render_info_bar_compact(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  KEYBINDINGS BAR
// ══════════════════════════════════════════════════════════

fn render_keybindings(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  HELP OVERLAY
// ══════════════════════════════════════════════════════════

fn render_help_overlay(f: &mut Frame, area: Rect, t: &Theme) {
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

// ══════════════════════════════════════════════════════════
//  MARK-COMPLETE CONFIRMATION DIALOG
// ══════════════════════════════════════════════════════════

fn render_complete_confirm_dialog(f: &mut Frame, area: Rect, app: &App, agent_id: usize) {
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

// ══════════════════════════════════════════════════════════
//  KILL CONFIRMATION DIALOG
// ══════════════════════════════════════════════════════════

fn render_kill_confirm_dialog(f: &mut Frame, area: Rect, app: &App, agent_id: usize) {
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

// ══════════════════════════════════════════════════════════
//  QUIT CONFIRMATION DIALOG
// ══════════════════════════════════════════════════════════

fn render_quit_confirm_dialog(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  POLL ERROR BANNER
// ══════════════════════════════════════════════════════════

fn render_poll_error_banner(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  SEARCH BAR
// ══════════════════════════════════════════════════════════

fn render_search_bar(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  JUMP BAR
// ══════════════════════════════════════════════════════════

fn render_jump_bar(f: &mut Frame, area: Rect, app: &App) {
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
            .map(|(id, is_agent)| {
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
    let compact_rows = term_rows < 40;

    // Main vertical layout chrome:
    //   Normal:  3+1+1+3+3+1 = 12  (title+tab+status_summary+gauges+info+keys)
    //   Compact: 3+1+1+1+1   = 7   (title+tab+status_summary+info+keys)
    let chrome = if compact_rows { 7 } else { 12 };
    let content_height = term_rows.saturating_sub(chrome);

    // Agent detail vertical layout:
    //   Length(3)  agent header
    //   Min(5)    output area
    let output_height = content_height.saturating_sub(3);

    // Output horizontal split:
    //   < 148 cols: no diagnostics panel, full width for output
    //   >= 148 cols: Min(40) output + Length(56) diagnostics panel
    let output_width = if term_cols < DIAGNOSTICS_PANEL_THRESHOLD {
        term_cols
    } else {
        term_cols.saturating_sub(DIAGNOSTICS_PANEL_WIDTH)
    };

    // The output block has Borders::ALL → subtract 2 from each dimension for inner area
    let inner_rows = output_height.saturating_sub(2);
    let inner_cols = output_width.saturating_sub(2);

    (inner_rows, inner_cols)
}

// ══════════════════════════════════════════════════════════
//  HELPERS
// ══════════════════════════════════════════════════════════

fn log_category_color(cat: LogCategory, t: &Theme) -> Color {
    match cat {
        LogCategory::System => t.muted,
        LogCategory::Incoming => t.bright,
        LogCategory::Deploy => t.bright,
        LogCategory::Complete => t.accent,
        LogCategory::Alert => t.danger,
        LogCategory::Poll => t.muted,
    }
}

fn phase_color(phase: AgentPhase, t: &Theme) -> Color {
    match phase {
        AgentPhase::Detecting | AgentPhase::Claiming | AgentPhase::Worktree => t.muted,
        AgentPhase::Implementing => t.bright,
        AgentPhase::Verifying | AgentPhase::Merging | AgentPhase::Closing | AgentPhase::Done => {
            t.accent
        }
    }
}

/// Render a compact phase step indicator: P0·P1·P2·[P3]·P4·P5·P6·P7
fn render_phase_indicator(phase: AgentPhase, t: &Theme) -> Vec<Span<'static>> {
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
            spans.push(Span::styled("·", Style::default().fg(t.muted)));
        }
        if *p == phase {
            spans.push(Span::styled(
                format!("[{}]", p.short()),
                Style::default()
                    .fg(phase_color(phase, t))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if *p < phase {
            spans.push(Span::styled(
                p.short().to_string(),
                Style::default().fg(t.muted),
            ));
        } else {
            spans.push(Span::styled(
                p.short().to_string(),
                Style::default().fg(Color::Rgb(35, 35, 45)),
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

// ══════════════════════════════════════════════════════════
//  ISSUE CREATION FORM
// ══════════════════════════════════════════════════════════

fn render_issue_creation_form(f: &mut Frame, area: Rect, app: &App) {
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

// ══════════════════════════════════════════════════════════
//  TESTS — Compact-mode breakpoints (obelisk-0ys)
// ══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::compute_pty_area;

    /// 80x24 — the classic "small terminal" target.
    /// Compact rows (<40) → chrome=7, no diagnostics panel (<148 cols).
    #[test]
    fn pty_area_80x24() {
        let (rows, cols) = compute_pty_area(80, 24);
        // content=17, output=14, inner=12×78
        assert_eq!(rows, 12);
        assert_eq!(cols, 78);
    }

    /// 120x40 — normal chrome, but still below the diagnostics breakpoint.
    #[test]
    fn pty_area_120x40() {
        let (rows, cols) = compute_pty_area(120, 40);
        // content=28, output=25, inner=23×118
        assert_eq!(rows, 23);
        assert_eq!(cols, 118);
    }

    /// 100x30 — compact rows, no diagnostics panel.
    #[test]
    fn pty_area_100x30() {
        let (rows, cols) = compute_pty_area(100, 30);
        // compact chrome=7, content=23, output=20, inner=18×98
        assert_eq!(rows, 18);
        assert_eq!(cols, 98);
    }

    /// 200x50 — large terminal: normal chrome, wider diagnostics present.
    #[test]
    fn pty_area_large_terminal() {
        let (rows, cols) = compute_pty_area(200, 50);
        // chrome=12, content=38, output=35, inner=33×(200-56-2)=142
        assert_eq!(rows, 33);
        assert_eq!(cols, 142);
    }

    /// Very small terminal — saturating_sub prevents underflow.
    #[test]
    fn pty_area_tiny_terminal() {
        let (rows, cols) = compute_pty_area(20, 10);
        // compact chrome=7, content=3, output=0, inner=0×18
        // (saturating_sub keeps it at 0, not negative)
        assert!(rows <= 1, "rows should be ≤1 at 10 rows high, got {rows}");
        assert!(cols > 0, "cols should be positive at 20 cols wide, got {cols}");
    }

    /// The wider diagnostics panel stays hidden until 148 cols.
    #[test]
    fn pty_area_sidebar_threshold() {
        let (_, cols_147) = compute_pty_area(147, 50);
        let (_, cols_148) = compute_pty_area(148, 50);
        // 147: no diagnostics → inner = 147-2 = 145
        // 148: diagnostics visible → inner = 148-56-2 = 90
        assert_eq!(cols_147, 145);
        assert_eq!(cols_148, 90);
    }

    /// At exactly 39 rows, compact mode; at 40, normal mode.
    #[test]
    fn pty_area_compact_row_threshold() {
        let (rows_39, _) = compute_pty_area(80, 39);
        let (rows_40, _) = compute_pty_area(80, 40);
        // 39: compact chrome=7, content=32, output=29, inner=27
        // 40: normal chrome=12, content=28, output=25, inner=23
        assert_eq!(rows_39, 27);
        assert_eq!(rows_40, 23);
        // Compact mode gives MORE content rows because chrome is smaller
        assert!(rows_39 > rows_40);
    }
}
