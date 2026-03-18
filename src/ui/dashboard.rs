use super::*;
use super::helpers::*;

pub(super) fn render_dashboard(f: &mut Frame, area: Rect, app: &mut App) {
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
            let msg_str: &str = msg;
            let alert = Paragraph::new(Line::from(Span::styled(
                msg_str,
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
        let mut types: Vec<&str> = app.type_filter.iter().map(|s: &String| s.as_str()).collect();
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
    let labels_str = task.labels.as_ref().map(|l: &Vec<String>| l.join(", ")).unwrap_or_default();
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
