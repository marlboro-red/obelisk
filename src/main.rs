mod app;
mod notify;
mod runtime;
mod templates;
mod types;
mod ui;

use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;
use types::{AppEvent, LogCategory, View, Focus};

const TICK_RATE_MS: u64 = 100;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    // Set initial PTY size from actual terminal dimensions
    let init_size = terminal.size()?;
    let (init_rows, init_cols) = ui::compute_pty_area(init_size.width, init_size.height);
    app.last_pty_size = (init_rows, init_cols);

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Terminal event reader (blocking I/O, so use spawn_blocking)
    let tx_term = tx.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            if event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    if tx_term.send(AppEvent::Terminal(ev)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Tick timer
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(TICK_RATE_MS));
        loop {
            interval.tick().await;
            if tx_tick.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });

    // Poller
    let tx_poll = tx.clone();
    tokio::spawn(async move {
        loop {
            match runtime::poll_ready().await {
                Ok(tasks) => {
                    if tx_poll.send(AppEvent::PollResult(tasks)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx_poll.send(AppEvent::PollFailed(e.to_string()));
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

    // Startup worktree scan — warn if orphaned agent worktrees exist from a previous session
    let tx_wt = tx.clone();
    tokio::spawn(async move {
        let worktrees = runtime::scan_agent_worktrees().await;
        if !worktrees.is_empty() {
            let paths: Vec<String> = worktrees.into_iter().map(|(p, _)| p).collect();
            let _ = tx_wt.send(AppEvent::WorktreeOrphans(paths));
        }
    });

    // Initial render before entering event loop
    let mut prev_view = app.active_view;
    let mut prev_term_size = terminal.size()?;
    terminal.draw(|f| ui::render(f, &mut app))?;

    // Main loop — only render on tick/input, batch data events between frames
    loop {
        let mut needs_render = false;

        // Wait for at least one event
        match rx.recv().await {
            Some(event) => {
                if matches!(event, AppEvent::Tick | AppEvent::Terminal(_)) {
                    needs_render = true;
                }
                process_event(&mut app, event, &tx);
            }
            None => break,
        }

        // Drain all remaining pending events without blocking
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AppEvent::Tick | AppEvent::Terminal(_)) {
                needs_render = true;
            }
            process_event(&mut app, event, &tx);
            if app.should_quit {
                break;
            }
        }

        if app.should_quit {
            break;
        }

        // Only render on tick (~10 FPS) or user input — not on every data event
        if needs_render {
            // Force full redraw when switching views to prevent artifacts
            if app.active_view != prev_view {
                terminal.clear()?;
                prev_view = app.active_view;
            }
            // Sync PTY sizes to match the actual output panel area
            let term_size = terminal.size()?;
            let (pty_rows, pty_cols) = ui::compute_pty_area(term_size.width, term_size.height);
            app.sync_pty_sizes(pty_rows, pty_cols);
            // Force full redraw on terminal resize so ratatui repaints every
            // cell and does not leave stale content from the old dimensions.
            if term_size != prev_term_size {
                terminal.clear()?;
                prev_term_size = term_size;
            }

            terminal.draw(|f| ui::render(f, &mut app))?;
        }
    }

    // Save settings and kill all running agent processes before exiting
    app.save_config();
    app.kill_all_agents();

    // Persist session record
    app.save_session();

    Ok(())
}

fn process_event(
    app: &mut App,
    event: AppEvent,
    tx: &mpsc::UnboundedSender<AppEvent>,
) {
    match event {
        AppEvent::Terminal(Event::Key(key)) => {
            handle_key(app, key, tx);
        }
        AppEvent::Terminal(Event::Mouse(mouse)) => {
            if app.mouse_enabled {
                handle_mouse(app, mouse, tx);
            }
        }
        AppEvent::Tick => {
            app.on_tick();

            if app.auto_spawn {
                while let Some(req) = app.get_auto_spawn_info() {
                    tokio::spawn(spawn_agent_process(tx.clone(), req));
                }
            }

            // Auto-fill split panes when in split view (~every 2s = 20 ticks)
            if app.active_view == View::SplitPane && app.frame_count % 20 == 0 {
                app.auto_fill_split_panes();
            }
            // Periodic diff refresh (~every 3s = 30 ticks at 100ms)
            if app.show_diff_panel
                && app.active_view == View::AgentDetail
                && app.frame_count.saturating_sub(app.diff_last_poll_frame) >= 30
            {
                if let Some(wt_path) = app.selected_agent_worktree() {
                    let agent_id = app.selected_agent_id.unwrap();
                    app.diff_last_poll_frame = app.frame_count;
                    let tx_diff = tx.clone();
                    tokio::spawn(async move {
                        let diff = runtime::poll_worktree_diff(&wt_path).await;
                        let _ = tx_diff.send(AppEvent::DiffResult { agent_id, diff });
                    });
                }
            }
        }
        AppEvent::PollResult(tasks) => {
            app.on_poll_result(tasks);
        }
        AppEvent::PollFailed(error) => {
            app.on_poll_failed(error);
        }
        AppEvent::AgentOutput { agent_id, line } => {
            app.on_agent_output(agent_id, line);
        }
        AppEvent::AgentExited {
            agent_id,
            exit_code,
        } => {
            app.on_agent_exited(agent_id, exit_code);
        }
        AppEvent::AgentPid { agent_id, pid } => {
            app.on_agent_pid(agent_id, pid);
        }
        AppEvent::AgentPtyData { agent_id, data } => {
            app.on_agent_pty_data(agent_id, &data);
        }
        AppEvent::AgentPtyReady { agent_id, handle } => {
            app.on_agent_pty_ready(agent_id, handle);
        }
        AppEvent::WorktreeOrphans(paths) => {
            app.on_worktree_orphans(paths);
        }
        AppEvent::WorktreeCleaned { cleaned, failed } => {
            app.on_worktree_cleaned(cleaned, failed);
        }
        AppEvent::DiffResult { agent_id, diff } => {
            app.on_diff_result(agent_id, diff);
        }
        AppEvent::Terminal(Event::Resize(_, _)) => {
            // PTY resize is handled in the render loop via sync_pty_sizes,
            // which runs before every draw and avoids double-resize here.
        }
        AppEvent::Terminal(_) => {}
    }
}

fn handle_key(
    app: &mut App,
    key: event::KeyEvent,
    tx: &mpsc::UnboundedSender<AppEvent>,
) {
    // On Windows, crossterm emits both Press and Release events; only handle Press.
    if key.kind != KeyEventKind::Press {
        return;
    }

    // ── Interactive mode: forward everything to PTY except Ctrl+] (detach) ──
    if app.interactive_mode {
        // Ctrl+] = detach from interactive mode (classic telnet escape)
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(']') {
            app.interactive_mode = false;
            app.log(LogCategory::System, "Detached from interactive session".into());
            return;
        }
        // Forward keystroke to the agent's PTY
        if let Some(bytes) = key_to_pty_bytes(&key) {
            app.write_to_agent(&bytes);
        }
        return;
    }

    // ── Normal TUI mode ──

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    // '?' toggles help overlay; Esc also closes it if open
    if key.code == KeyCode::Char('?') {
        app.show_help = !app.show_help;
        return;
    }
    if app.show_help {
        if key.code == KeyCode::Esc {
            app.show_help = false;
        }
        return;
    }

    // ── Search mode: intercept keys when search bar is active in agent detail ──
    if app.search_active && app.active_view == View::AgentDetail {
        match key.code {
            KeyCode::Esc => {
                app.search_active = false;
                app.search_query.clear();
                app.search_matches.clear();
            }
            KeyCode::Char('n') => app.search_next(),
            KeyCode::Char('N') => app.search_prev(),
            KeyCode::Backspace => {
                app.search_query.pop();
                app.update_search_matches();
            }
            KeyCode::Char(c) => {
                app.search_query.push(c);
                app.update_search_matches();
            }
            _ => {}
        }
        return;
    }

    // ── Jump-to-issue mode: intercept keys when jump bar is active ──
    if app.jump_active {
        match key.code {
            KeyCode::Esc => {
                app.jump_active = false;
                app.jump_query.clear();
            }
            KeyCode::Enter => {
                let found = app.jump_execute();
                app.jump_active = false;
                if !found && !app.jump_query.is_empty() {
                    app.log(
                        crate::types::LogCategory::System,
                        format!("No issue matching \"{}\"", app.jump_query),
                    );
                }
                app.jump_query.clear();
            }
            KeyCode::Backspace => {
                app.jump_query.pop();
            }
            KeyCode::Char(c) => {
                app.jump_query.push(c);
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => {
            if app.active_view == View::AgentDetail || app.active_view == View::SplitPane {
                app.interactive_mode = false;
                app.search_active = false;
                app.search_query.clear();
                app.search_matches.clear();
                app.active_view = View::Dashboard;
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Esc => {
            if app.active_view != View::Dashboard {
                app.interactive_mode = false;
                app.search_active = false;
                app.search_query.clear();
                app.search_matches.clear();
                app.active_view = View::Dashboard;
            }
        }
        KeyCode::Char('1') => { app.interactive_mode = false; app.active_view = View::Dashboard; }
        KeyCode::Char('2') => {
            if app.selected_agent_id.is_none() && !app.agents.is_empty() {
                app.selected_agent_id = Some(app.agents[0].id);
            }
            app.active_view = View::AgentDetail;
        }
        KeyCode::Char('3') => app.active_view = View::EventLog,
        KeyCode::Char('4') => app.active_view = View::History,
        KeyCode::Char('5') => {
            app.auto_fill_split_panes();
            app.active_view = View::SplitPane;
        }

        // ── Interactive mode: press 'i' in agent detail to attach ──
        KeyCode::Char('i') if app.active_view == View::AgentDetail => {
            if let Some(agent_id) = app.selected_agent_id {
                if app.pty_states.contains_key(&agent_id) {
                    // Check agent is still alive
                    let alive = app.agents.iter().any(|a| {
                        a.id == agent_id
                            && matches!(
                                a.status,
                                crate::types::AgentStatus::Starting
                                    | crate::types::AgentStatus::Running
                            )
                    });
                    if alive {
                        app.interactive_mode = true;
                        app.log(
                            LogCategory::System,
                            format!("Attached to AGENT-{:02} — Ctrl+] to detach", agent_id),
                        );
                    }
                }
            }
        }

        // ── Split-pane view keys ──
        KeyCode::Up if app.active_view == View::SplitPane => {
            app.split_pane_scroll_up();
        }
        KeyCode::Down if app.active_view == View::SplitPane => {
            app.split_pane_scroll_down();
        }
        KeyCode::Tab if app.active_view == View::SplitPane => {
            let count = app.split_pane_count(160);
            app.split_pane_focus = (app.split_pane_focus + 1) % count.max(1);
        }
        KeyCode::Enter if app.active_view == View::SplitPane => {
            app.split_pane_enter_detail();
        }
        KeyCode::Char('g') if app.active_view == View::SplitPane => {
            app.toggle_pin_split_pane();
        }
        KeyCode::Up => app.navigate_up(),
        KeyCode::Down => app.navigate_down(),
        KeyCode::PageUp if app.active_view == View::AgentDetail => app.page_up(),
        KeyCode::PageDown if app.active_view == View::AgentDetail => app.page_down(),
        KeyCode::PageUp if app.active_view == View::History => {
            app.history_scroll = app.history_scroll.saturating_sub(10);
        }
        KeyCode::PageDown if app.active_view == View::History => {
            app.history_scroll = (app.history_scroll + 10).min(app.history_sessions.len().saturating_sub(1));
        }
        KeyCode::Home if app.active_view == View::AgentDetail => {
            app.agent_output_scroll = Some(0);
        }
        KeyCode::End if app.active_view == View::AgentDetail => {
            app.agent_output_scroll = None; // re-engage auto-follow
        }
        KeyCode::Char('/') if app.active_view == View::AgentDetail => {
            if !app.interactive_mode {
                app.search_active = true;
                app.search_query.clear();
                app.search_matches.clear();
                app.search_current_idx = 0;
                app.update_search_matches();
            }
        }
        KeyCode::Char('/') if app.active_view == View::Dashboard => {
            app.jump_active = true;
            app.jump_query.clear();
        }
        KeyCode::Char('k') if app.active_view == View::AgentDetail => {
            if let Some(agent_id) = app.selected_agent_id {
                if let Some((_, worktree)) = app.kill_agent(agent_id) {
                    if let Some(worktree_path) = worktree {
                        // Derive branch name: worktree path is "../worktree-{branch}"
                        let branch = std::path::Path::new(&worktree_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.strip_prefix("worktree-"))
                            .unwrap_or("")
                            .to_string();
                        let tx_kill = tx.clone();
                        tokio::spawn(async move {
                            let mut cleaned = Vec::new();
                            let mut failed = Vec::new();
                            match runtime::cleanup_worktree(&worktree_path, &branch).await {
                                Ok(()) => cleaned.push(worktree_path),
                                Err(_) => failed.push(worktree_path),
                            }
                            let _ = tx_kill.send(AppEvent::WorktreeCleaned { cleaned, failed });
                        });
                    }
                }
            }
        }
        KeyCode::Char('r') if app.active_view == View::AgentDetail => {
            if let Some(agent_id) = app.selected_agent_id {
                if let Some(req) = app.retry_agent(agent_id) {
                    tokio::spawn(spawn_agent_process(tx.clone(), req));
                }
            }
        }
        KeyCode::Char('j') if app.active_view != View::AgentDetail => app.navigate_down(),
        KeyCode::Char('k') if app.active_view != View::AgentDetail => app.navigate_up(),
        KeyCode::Tab => app.toggle_focus(),
        KeyCode::Enter => app.enter_pressed(),
        KeyCode::Char('s') if app.active_view == View::Dashboard => {
            if let Some(req) = app.get_spawn_info() {
                tokio::spawn(spawn_agent_process(tx.clone(), req));
            }
        }
        KeyCode::Char('p') => {
            app.log(LogCategory::Poll, "Manual scan initiated".into());
            app.poll_countdown = 0.0;
            let tx = tx.clone();
            tokio::spawn(async move {
                match runtime::poll_ready().await {
                    Ok(tasks) => {
                        let _ = tx.send(AppEvent::PollResult(tasks));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::PollFailed(e.to_string()));
                    }
                }
            });
        }
        KeyCode::Char('r') if app.active_view == View::Dashboard => {
            app.selected_runtime = app.selected_runtime.next();
            app.log(
                LogCategory::System,
                format!(
                    "Runtime switched to {} [{}]",
                    app.selected_runtime.name(),
                    app.selected_model()
                ),
            );
        }
        KeyCode::Char('m') if app.active_view == View::Dashboard => {
            app.cycle_model();
            app.log(
                LogCategory::System,
                format!(
                    "Model switched to {}",
                    app.selected_model()
                ),
            );
        }
        KeyCode::Char('a') if app.active_view == View::Dashboard => {
            app.auto_spawn = !app.auto_spawn;
            app.log(
                LogCategory::System,
                format!(
                    "Auto-spawn {}",
                    if app.auto_spawn { "ENABLED" } else { "DISABLED" }
                ),
            );
        }
        KeyCode::Char('z') if app.active_view == View::Dashboard => {
            app.auto_exit_on_completion = !app.auto_exit_on_completion;
            app.log(
                LogCategory::System,
                format!(
                    "Auto-exit on completion {}",
                    if app.auto_exit_on_completion { "ENABLED" } else { "DISABLED" }
                ),
            );
        }
        KeyCode::Char('t') if app.active_view == View::Dashboard => {
            const PRESETS: &[u64] = &[300, 900, 1800, 3600, 0];
            let next = PRESETS
                .iter()
                .position(|&x| x == app.agent_timeout_secs)
                .map(|i| PRESETS[(i + 1) % PRESETS.len()])
                .unwrap_or(1800);
            app.agent_timeout_secs = next;
            let label = if next == 0 {
                "DISABLED".to_string()
            } else {
                App::format_elapsed(next)
            };
            app.log(LogCategory::System, format!("Agent timeout: {}", label));
        }
        KeyCode::Char('n') if !app.search_active => {
            app.notifications_enabled = !app.notifications_enabled;
            app.log(
                LogCategory::System,
                format!(
                    "Notifications {}",
                    if app.notifications_enabled { "ENABLED" } else { "DISABLED" }
                ),
            );
        }
        KeyCode::Char('M') if app.active_view == View::Dashboard => {
            app.mouse_enabled = !app.mouse_enabled;
            app.log(
                LogCategory::System,
                format!(
                    "Mouse support {}",
                    if app.mouse_enabled { "ENABLED" } else { "DISABLED" }
                ),
            );
        }
        // Sort mode cycling — 'f' on Dashboard with ReadyQueue focus
        KeyCode::Char('f')
            if app.active_view == View::Dashboard && app.focus == Focus::ReadyQueue =>
        {
            app.cycle_sort_mode();
        }
        // Type filter cycling — 'F' (Shift+f) on Dashboard with ReadyQueue focus
        KeyCode::Char('F')
            if app.active_view == View::Dashboard && app.focus == Focus::ReadyQueue =>
        {
            app.cycle_type_filter();
        }
        // Agent status filter cycling — 'f' on Dashboard with AgentList focus
        KeyCode::Char('f')
            if app.active_view == View::Dashboard && app.focus == Focus::AgentList =>
        {
            app.cycle_agent_status_filter();
        }
        KeyCode::Char('x')
            if app.active_view == View::Dashboard
                && app.focus == Focus::AgentList =>
        {
            if let Some(msg) = app.dismiss_selected_agent() {
                app.log(LogCategory::System, msg);
            }
        }
        KeyCode::Char('X')
            if app.active_view == View::Dashboard
                && app.focus == Focus::AgentList =>
        {
            let count = app.dismiss_all_finished();
            if count > 0 {
                app.log(
                    LogCategory::System,
                    format!("{} finished agent(s) dismissed", count),
                );
            }
        }
        KeyCode::Char('c') if app.active_view == View::Dashboard => {
            app.log(LogCategory::System, "Scanning for orphaned worktrees...".into());
            let active_ids = app.active_task_ids();
            let tx_clean = tx.clone();
            tokio::spawn(async move {
                let worktrees = runtime::scan_agent_worktrees().await;
                // Filter: keep only worktrees not associated with a currently active agent
                let orphans: Vec<(String, String)> = worktrees
                    .into_iter()
                    .filter(|(path, _branch)| {
                        let task_id = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.strip_prefix("worktree-"))
                            .unwrap_or("");
                        !active_ids.contains(task_id)
                    })
                    .collect();
                let mut cleaned = Vec::new();
                let mut failed = Vec::new();
                for (path, branch) in orphans {
                    match runtime::cleanup_worktree(&path, &branch).await {
                        Ok(()) => cleaned.push(path),
                        Err(_) => failed.push(path),
                    }
                }
                let _ = tx_clean.send(AppEvent::WorktreeCleaned { cleaned, failed });
            });
        }
        // 'd' toggles diff panel in Agent Detail view
        KeyCode::Char('d') if app.active_view == View::AgentDetail => {
            app.toggle_diff_panel();
            if app.show_diff_panel {
                // Fire immediate diff poll
                if let Some(wt_path) = app.selected_agent_worktree() {
                    let agent_id = app.selected_agent_id.unwrap();
                    let tx_diff = tx.clone();
                    tokio::spawn(async move {
                        let diff = runtime::poll_worktree_diff(&wt_path).await;
                        let _ = tx_diff.send(AppEvent::DiffResult { agent_id, diff });
                    });
                }
            }
        }
        // Diff panel scroll (Ctrl+Up/Down to not conflict with output scroll)
        KeyCode::Char('J') if app.active_view == View::AgentDetail && app.show_diff_panel => {
            app.diff_scroll = app.diff_scroll.saturating_add(1);
        }
        KeyCode::Char('K') if app.active_view == View::AgentDetail && app.show_diff_panel => {
            app.diff_scroll = app.diff_scroll.saturating_sub(1);
        }
        KeyCode::Left if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i > 0 {
                        app.interactive_mode = false;
                        app.search_active = false;
                        app.search_query.clear();
                        app.search_matches.clear();
                        app.selected_agent_id = Some(app.agents[i - 1].id);
                        app.agent_output_scroll = None;
                        app.diff_data = None;
                        app.diff_scroll = 0;
                        app.diff_last_poll_frame = 0;
                    }
                }
            }
        }
        KeyCode::Right if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i + 1 < app.agents.len() {
                        app.interactive_mode = false;
                        app.search_active = false;
                        app.search_query.clear();
                        app.search_matches.clear();
                        app.selected_agent_id = Some(app.agents[i + 1].id);
                        app.agent_output_scroll = None;
                        app.diff_data = None;
                        app.diff_scroll = 0;
                        app.diff_last_poll_frame = 0;
                    }
                }
            }
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if app.max_concurrent < 20 {
                app.max_concurrent += 1;
                app.log(
                    LogCategory::System,
                    format!("Max concurrent agents: {}", app.max_concurrent),
                );
            }
        }
        KeyCode::Char('-') => {
            if app.max_concurrent > 1 {
                app.max_concurrent -= 1;
                app.log(
                    LogCategory::System,
                    format!("Max concurrent agents: {}", app.max_concurrent),
                );
            }
        }
        _ => {}
    }
}

fn handle_mouse(
    app: &mut App,
    mouse: event::MouseEvent,
    _tx: &mpsc::UnboundedSender<AppEvent>,
) {
    // Don't process mouse events in interactive mode
    if app.interactive_mode {
        return;
    }

    let col = mouse.column;
    let row = mouse.row;

    // Helper: check if (col, row) is inside a Rect
    let in_rect = |r: &ratatui::layout::Rect| -> bool {
        col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
    };

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // ── Tab bar clicks ──
            if let Some(tab_area) = app.layout_areas.tab_bar {
                if in_rect(&tab_area) {
                    // Tabs are laid out evenly with dividers
                    // Tab titles: " 1:DASHBOARD ", " 2:AGENTS ", " 3:EVENT LOG ", " 4:HISTORY ", " 5:SPLIT "
                    let rel_x = col.saturating_sub(tab_area.x) as usize;
                    // Each tab is ~15 chars + 3 char divider; estimate positions
                    // Use proportional split: divide width by 5
                    let tab_width = tab_area.width as usize / 5;
                    if tab_width > 0 {
                        let tab_idx = (rel_x / tab_width).min(4);
                        match tab_idx {
                            0 => { app.interactive_mode = false; app.active_view = View::Dashboard; }
                            1 => {
                                if app.selected_agent_id.is_none() && !app.agents.is_empty() {
                                    app.selected_agent_id = Some(app.agents[0].id);
                                }
                                app.active_view = View::AgentDetail;
                            }
                            2 => app.active_view = View::EventLog,
                            3 => app.active_view = View::History,
                            4 => {
                                app.auto_fill_split_panes();
                                app.active_view = View::SplitPane;
                            }
                            _ => {}
                        }
                    }
                    return;
                }
            }

            // ── Dashboard: click on ready queue items ──
            if app.active_view == View::Dashboard {
                if let Some(queue_area) = app.layout_areas.ready_queue {
                    if in_rect(&queue_area) {
                        app.focus = Focus::ReadyQueue;
                        // Account for border (1 row top)
                        let inner_y = row.saturating_sub(queue_area.y + 1) as usize;
                        let filtered_len = app.filtered_tasks().len();
                        if inner_y < filtered_len {
                            app.task_list_state.select(Some(inner_y));
                        }
                        return;
                    }
                }

                if let Some(agent_area) = app.layout_areas.agent_panel {
                    if in_rect(&agent_area) {
                        app.focus = Focus::AgentList;
                        // Account for border (1 row top)
                        let inner_y = row.saturating_sub(agent_area.y + 1) as usize;
                        let visible_len = app.filtered_agents().len();
                        if inner_y < visible_len {
                            app.agent_list_state.select(Some(inner_y));
                        }
                        return;
                    }
                }
            }

            // ── Dashboard: double-click to enter agent detail ──
            // (single click selects, Enter enters — consistent with keyboard)

            // ── Split pane: click to focus a pane ──
            if app.active_view == View::SplitPane {
                for (slot, pane_rect) in app.layout_areas.split_panes.iter().enumerate() {
                    if let Some(rect) = pane_rect {
                        if in_rect(rect) {
                            app.split_pane_focus = slot;
                            return;
                        }
                    }
                }
            }
        }

        // ── Scroll wheel ──
        MouseEventKind::ScrollUp => {
            match app.active_view {
                View::Dashboard => {
                    if let Some(queue_area) = app.layout_areas.ready_queue {
                        if in_rect(&queue_area) {
                            app.focus = Focus::ReadyQueue;
                            app.navigate_up();
                            return;
                        }
                    }
                    if let Some(agent_area) = app.layout_areas.agent_panel {
                        if in_rect(&agent_area) {
                            app.focus = Focus::AgentList;
                            app.navigate_up();
                            return;
                        }
                    }
                }
                View::AgentDetail => {
                    if let Some(output_area) = app.layout_areas.agent_detail_output {
                        if in_rect(&output_area) {
                            // Scroll output up by 3 lines
                            for _ in 0..3 {
                                app.navigate_up();
                            }
                            return;
                        }
                    }
                }
                View::EventLog => {
                    app.navigate_up();
                }
                View::History => {
                    app.navigate_up();
                }
                View::SplitPane => {
                    for (slot, pane_rect) in app.layout_areas.split_panes.iter().enumerate() {
                        if let Some(rect) = pane_rect {
                            if in_rect(rect) {
                                app.split_pane_focus = slot;
                                app.split_pane_scroll_up();
                                return;
                            }
                        }
                    }
                }
            }
        }

        MouseEventKind::ScrollDown => {
            match app.active_view {
                View::Dashboard => {
                    if let Some(queue_area) = app.layout_areas.ready_queue {
                        if in_rect(&queue_area) {
                            app.focus = Focus::ReadyQueue;
                            app.navigate_down();
                            return;
                        }
                    }
                    if let Some(agent_area) = app.layout_areas.agent_panel {
                        if in_rect(&agent_area) {
                            app.focus = Focus::AgentList;
                            app.navigate_down();
                            return;
                        }
                    }
                }
                View::AgentDetail => {
                    if let Some(output_area) = app.layout_areas.agent_detail_output {
                        if in_rect(&output_area) {
                            // Scroll output down by 3 lines
                            for _ in 0..3 {
                                app.navigate_down();
                            }
                            return;
                        }
                    }
                }
                View::EventLog => {
                    app.navigate_down();
                }
                View::History => {
                    app.navigate_down();
                }
                View::SplitPane => {
                    for (slot, pane_rect) in app.layout_areas.split_panes.iter().enumerate() {
                        if let Some(rect) = pane_rect {
                            if in_rect(rect) {
                                app.split_pane_focus = slot;
                                app.split_pane_scroll_down();
                                return;
                            }
                        }
                    }
                }
            }
        }

        _ => {} // Ignore other mouse events (drag, move, etc.)
    }
}

/// Convert a crossterm key event to raw PTY bytes.
/// This maps keys to the escape sequences a real terminal would send.
fn key_to_pty_bytes(key: &event::KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    Some(match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+A..Z → 0x01..0x1A
                let byte = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a').wrapping_add(1);
                vec![byte]
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        _ => return None,
    })
}

/// Spawn an agent in a PTY. Sets up reader + exit watcher tasks,
/// sends the PtyHandle to the main thread, and injects the initial prompt.
async fn spawn_agent_process(
    tx: mpsc::UnboundedSender<AppEvent>,
    req: app::SpawnRequest,
) {
    let agent_id = req.agent_id;
    let runtime = req.runtime;
    let model = req.model.clone();
    let system_prompt = req.system_prompt.clone();
    let user_prompt = req.user_prompt.clone();
    let task = req.task.clone();
    let pty_rows = req.pty_rows;
    let pty_cols = req.pty_cols;

    // PTY creation is blocking — run in spawn_blocking
    let spawn_result = tokio::task::spawn_blocking(move || {
        runtime::spawn_agent_pty(runtime, &model, &task, &system_prompt, &user_prompt, pty_rows, pty_cols)
    })
    .await;

    let (handle, reader, child) = match spawn_result {
        Ok(Ok(tuple)) => tuple,
        Ok(Err(e)) => {
            let _ = tx.send(AppEvent::AgentOutput {
                agent_id,
                line: format!("[ERROR] PTY spawn failed: {}", e),
            });
            let _ = tx.send(AppEvent::AgentExited {
                agent_id,
                exit_code: Some(1),
            });
            return;
        }
        Err(e) => {
            let _ = tx.send(AppEvent::AgentOutput {
                agent_id,
                line: format!("[ERROR] spawn_blocking panicked: {}", e),
            });
            let _ = tx.send(AppEvent::AgentExited {
                agent_id,
                exit_code: Some(1),
            });
            return;
        }
    };

    // Record PID
    if let Some(pid) = child.process_id() {
        let _ = tx.send(AppEvent::AgentPid { agent_id, pid });
    }

    // Send handle to main thread
    let _ = tx.send(AppEvent::AgentPtyReady {
        agent_id,
        handle,
    });

    // Reader task: read raw bytes from PTY, send as events
    let tx_reader = tx.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx_reader
                        .send(AppEvent::AgentPtyData {
                            agent_id,
                            data: buf[..n].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Exit watcher
    let tx_exit = tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut child = child;
        let status = child.wait();
        let exit_code = match status {
            Ok(s) => {
                if s.success() {
                    Some(0)
                } else {
                    Some(1)
                }
            }
            Err(_) => Some(1),
        };
        let _ = tx_exit.send(AppEvent::AgentExited {
            agent_id,
            exit_code,
        });
    });
}
