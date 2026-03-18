use super::*;

pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

pub(super) fn log_category_color(cat: LogCategory, t: &Theme) -> Color {
    match cat {
        LogCategory::System => t.muted,
        LogCategory::Incoming => t.bright,
        LogCategory::Deploy => t.bright,
        LogCategory::Complete => t.accent,
        LogCategory::Alert => t.danger,
        LogCategory::Poll => t.muted,
    }
}

pub(super) fn phase_color(phase: AgentPhase, t: &Theme) -> Color {
    match phase {
        AgentPhase::Detecting | AgentPhase::Claiming | AgentPhase::Worktree => t.muted,
        AgentPhase::Implementing => t.bright,
        AgentPhase::Verifying | AgentPhase::Merging | AgentPhase::Closing | AgentPhase::Done => {
            t.accent
        }
    }
}

/// Render a compact phase step indicator: P0·P1·P2·[P3]·P4·P5·P6·P7
pub(super) fn render_phase_indicator(phase: AgentPhase, t: &Theme) -> Vec<Span<'static>> {
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

/// Map vt100 color to ratatui Color.
pub(super) fn vt100_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Returns (age_label, age_color) based on how old the issue is.
/// Color bands: <1d neutral, 1-3d yellow, 3-7d orange, 7d+ red.
pub(super) fn age_badge(created_at: Option<&str>, t: &Theme) -> (String, Color) {
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

pub(super) fn dep_status_color(status: &str, t: &Theme) -> Color {
    match status {
        "closed" => t.accent,
        "in_progress" => t.info,
        "blocked" => t.danger,
        "deferred" => t.warn,
        _ => t.muted, // "open" and others
    }
}

pub(super) fn dep_status_symbol(status: &str) -> &'static str {
    match status {
        "closed" => "✓",
        "in_progress" => "▶",
        "blocked" => "✗",
        "deferred" => "◌",
        _ => "○", // "open"
    }
}

/// Compute the inner (rows, cols) of the terminal output panel given the full
/// terminal size. This mirrors the layout chain: main → agent_detail → output_block.
pub fn compute_pty_area(term_cols: u16, term_rows: u16) -> (u16, u16) {
    use super::agent_detail::{DIAGNOSTICS_PANEL_WIDTH, DIAGNOSTICS_PANEL_THRESHOLD};

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
