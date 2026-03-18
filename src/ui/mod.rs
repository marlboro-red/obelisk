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

mod helpers;
mod chromebar;
mod dashboard;
mod agent_detail;
mod event_log;
mod history;
mod split_pane;
mod worktree;
mod dep_graph;
mod dialogs;
mod forms;
#[cfg(test)]
mod tests;

pub use helpers::compute_pty_area;

use chromebar::*;
use dashboard::render_dashboard;
use agent_detail::render_agent_detail;
use event_log::render_event_log;
use history::render_history;
use split_pane::render_split_pane;
use worktree::render_worktree_overview;
use dep_graph::render_dep_graph;
use dialogs::*;
use forms::render_issue_creation_form;

// ══════════════════════════════════════════════════════════
//  THEMED BLOCK HELPER
// ══════════════════════════════════════════════════════════

pub(super) fn primary_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
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
