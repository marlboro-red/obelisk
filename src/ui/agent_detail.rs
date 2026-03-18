use super::*;
use super::helpers::*;
use super::dialogs::render_search_bar;

pub(super) const DIAGNOSTICS_PANEL_WIDTH: u16 = 56;
pub(super) const DIAGNOSTICS_PANEL_THRESHOLD: u16 = 148;

pub(super) fn render_agent_detail(f: &mut Frame, area: Rect, app: &mut App) {
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
        let scroll: usize = match app.agent_output_scroll {
            None => max_scroll,
            Some(pos) => pos.min(max_scroll),
        };

        let lines: Vec<Line> = agent
            .output
            .iter()
            .skip(scroll)
            .take(visible_height)
            .enumerate()
            .map(|(i, line): (usize, &String)| {
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

/// Render a vt100 screen without search highlighting (for split panes).
pub(super) fn render_vt100_screen_plain(f: &mut Frame, screen: &vt100::Screen, area: Rect) {
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
            Span::styled(" MODEL   ", Style::default().fg(t.muted)),
            Span::styled(
                format!(" {}", agent.model),
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
    // Token and cost data (only shown when available)
    if let Some(ref usage) = agent.usage {
        let total_input = usage.input_tokens + usage.cache_creation_tokens + usage.cache_read_tokens;
        lines.extend([
            Line::from(""),
            Line::from(vec![
                Span::styled(" IN TOK  ", Style::default().fg(t.muted)),
                Span::styled(
                    format!(" {}", crate::cost::format_tokens(total_input)),
                    Style::default().fg(t.bright),
                ),
            ]),
            Line::from(vec![
                Span::styled(" OUT TOK ", Style::default().fg(t.muted)),
                Span::styled(
                    format!(" {}", crate::cost::format_tokens(usage.output_tokens)),
                    Style::default().fg(t.bright),
                ),
            ]),
            Line::from(vec![
                Span::styled(" COST    ", Style::default().fg(t.muted)),
                Span::styled(
                    format!(" {}", crate::cost::format_cost(usage.cost_usd)),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]);
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

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
        let file: &String = file;
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
        let diff_line: &String = diff_line;
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
