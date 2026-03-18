mod app;
mod client;
mod cost;
mod daemon;
mod notify;
mod runtime;
mod templates;
mod theme;
mod types;
mod ui;

use app::App;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, warn};
use types::{AgentStatus, AppEvent, LogCategory, View, Focus};

const TICK_RATE_MS: u64 = 100;

fn print_usage() {
    eprintln!("Usage: obelisk [command]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  (none)            Launch the TUI dashboard");
    eprintln!("  serve, --daemon   Start headless daemon mode");
    eprintln!("  status            Show daemon status");
    eprintln!("  agents            List all agents");
    eprintln!("  spawn <issue-id>  Spawn agent for an issue");
    eprintln!("  kill <agent-id>   Kill a running agent");
    eprintln!("  stop              Stop the daemon");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Route to the appropriate mode based on CLI arguments
    match args.get(1).map(|s| s.as_str()) {
        None => run_tui().await,
        Some("serve") | Some("--daemon") => {
            init_logging();
            info!("obelisk daemon starting");
            daemon::run().await
        }
        Some("status") => {
            client::run(client::ClientCommand::Status).await
        }
        Some("agents") => {
            client::run(client::ClientCommand::Agents).await
        }
        Some("spawn") => {
            let issue_id = args.get(2).ok_or("usage: obelisk spawn <issue-id>")?;
            client::run(client::ClientCommand::Spawn {
                issue_id: issue_id.clone(),
            })
            .await
        }
        Some("kill") => {
            let id_str = args.get(2).ok_or("usage: obelisk kill <agent-id>")?;
            let agent_id: usize = id_str
                .parse()
                .map_err(|_| format!("invalid agent id: {}", id_str))?;
            client::run(client::ClientCommand::Kill { agent_id }).await
        }
        Some("stop") => {
            client::run(client::ClientCommand::Stop).await
        }
        Some("--help") | Some("-h") | Some("help") => {
            print_usage();
            Ok(())
        }
        Some(unknown) => {
            eprintln!("unknown command: {}", unknown);
            print_usage();
            std::process::exit(1);
        }
    }
}

/// Initialise file-based tracing with structured JSON output.
///
/// Log entries are written as one JSON object per line to
/// `.beads/logs/obelisk.log` (daily rolling).  Each line includes an ISO-8601
/// timestamp, level, structured fields, and message.  Callers should enter a
/// root span with `session_id` after initialisation so that every subsequent
/// log line is automatically tagged.
fn init_logging() {
    let log_dir = std::path::Path::new(".beads").join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "obelisk.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard so it lives for the process lifetime
    std::mem::forget(_guard);

    tracing_subscriber::fmt()
        .json()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .with_current_span(true)
        .with_span_list(false)
        .init();
}

async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    info!("obelisk starting");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        error!(%e, "obelisk fatal error");
        eprintln!("Error: {}", e);
    }

    info!("obelisk shutdown complete");

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();

    // Enter a root span carrying the session_id so every log line is tagged.
    let _session_span = info_span!("session", session_id = %app.session_id).entered();
    info!(
        runtime = %app.selected_runtime.name(),
        max_concurrent = app.max_concurrent,
        auto_spawn = app.auto_spawn,
        poll_interval_secs = app.poll_interval_secs,
        "session started"
    );

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

    // Shared poll interval — updated by config hot-reload, read by poller
    let shared_poll_interval = Arc::new(AtomicU64::new(app.poll_interval_secs));

    // Poller
    let tx_poll = tx.clone();
    let tx_blocked_poll = tx.clone();
    let tx_dep_poll = tx.clone();
    let poller_interval = Arc::clone(&shared_poll_interval);
    tokio::spawn(async move {
        let mut cycle: u64 = 0;
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
            // Poll blocked issues on the same cycle
            if let Ok(blocked) = runtime::poll_blocked().await {
                let _ = tx_blocked_poll.send(AppEvent::BlockedPollResult(blocked));
            }
            // Poll dep graph every 3rd cycle for dependency-aware auto-spawn
            if cycle % 3 == 0 {
                match runtime::poll_dep_graph().await {
                    Ok(nodes) => {
                        let _ = tx_dep_poll.send(AppEvent::DepGraphResult(nodes));
                    }
                    Err(e) => {
                        let _ = tx_dep_poll.send(AppEvent::DepGraphFailed(e));
                    }
                }
            }
            cycle += 1;
            let secs = poller_interval.load(Ordering::Relaxed);
            tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
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
                process_event(&mut app, event, &tx, &shared_poll_interval);
            }
            None => break,
        }

        // Drain all remaining pending events without blocking
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AppEvent::Tick | AppEvent::Terminal(_)) {
                needs_render = true;
            }
            process_event(&mut app, event, &tx, &shared_poll_interval);
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
    info!(
        total_completed = app.total_completed,
        total_failed = app.total_failed,
        agents_active = app.active_agent_count(),
        agents_total = app.agents.len(),
        uptime_secs = (chrono::Local::now() - app.session_started_at).num_seconds(),
        "session ending — shutting down"
    );
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
    shared_poll_interval: &Arc<AtomicU64>,
) {
    match event {
        AppEvent::Terminal(Event::Key(key)) => {
            handle_key(app, key, tx);
        }
        AppEvent::Tick => {
            app.on_tick();

            // Config hot-reload check (~every 2s)
            if app.check_config_reload() {
                shared_poll_interval.store(app.poll_interval_secs, Ordering::Relaxed);
            }

            if app.auto_spawn {
                while let Some(req) = app.get_auto_spawn_info() {
                    tokio::spawn(spawn_agent_process(tx.clone(), req));
                }
            }

            // Auto-fill split panes when in split view (~every 2s = 20 ticks)
            if app.active_view == View::SplitPane && app.frame_count.is_multiple_of(20) {
                app.auto_fill_split_panes();
            }
            // Periodic worktree scan (~every 5s = 50 ticks at 100ms) when panel is active
            if app.active_view == View::WorktreeOverview
                && app.frame_count.saturating_sub(app.worktree_last_scan_frame) >= 50
            {
                app.worktree_last_scan_frame = app.frame_count;
                let tx_wt = tx.clone();
                tokio::spawn(async move {
                    let worktrees = runtime::scan_agent_worktrees().await;
                    let _ = tx_wt.send(AppEvent::WorktreeScanned(worktrees));
                });
            }
            // Periodic dep graph refresh (~every 5s = 50 ticks at 100ms) when panel is active
            if app.active_view == View::DepGraph
                && app.frame_count.saturating_sub(app.dep_graph_last_poll_frame) >= 50
            {
                app.dep_graph_last_poll_frame = app.frame_count;
                let tx_dep = tx.clone();
                tokio::spawn(async move {
                    match runtime::poll_dep_graph().await {
                        Ok(nodes) => {
                            let _ = tx_dep.send(AppEvent::DepGraphResult(nodes));
                        }
                        Err(e) => {
                            let _ = tx_dep.send(AppEvent::DepGraphFailed(e));
                        }
                    }
                });
            }
            // Periodic diff refresh (~every 3s = 30 ticks at 100ms)
            if app.show_diff_panel
                && app.active_view == View::AgentDetail
                && app.frame_count.saturating_sub(app.diff_last_poll_frame) >= 30
            {
                if let Some(wt_path) = app.selected_agent_worktree() {
                    if let Some(agent_id) = app.selected_agent_id {
                        app.diff_last_poll_frame = app.frame_count;
                        let tx_diff = tx.clone();
                        tokio::spawn(async move {
                            let diff = runtime::poll_worktree_diff(&wt_path).await;
                            let _ = tx_diff.send(AppEvent::DiffResult { agent_id, diff });
                        });
                    }
                }
            }

            // Periodic issue status poll (~every 5s = 50 ticks at 100ms)
            // Polls `bd show <id> --json` for each running agent to detect completion
            // from the source of truth (beads) instead of PTY scraping.
            if app.frame_count.is_multiple_of(50) {
                let running: Vec<(usize, String)> = app
                    .agents
                    .iter()
                    .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
                    .map(|a| (a.id, a.task.id.clone()))
                    .collect();
                for (agent_id, task_id) in running {
                    let tx_status = tx.clone();
                    tokio::spawn(async move {
                        if let Ok(true) = runtime::poll_issue_status(&task_id).await {
                            let _ = tx_status.send(AppEvent::IssueStatusClosed { agent_id });
                        }
                    });
                }
            }
        }
        AppEvent::PollResult(tasks) => {
            debug!(
                ready_count = tasks.len(),
                event = "poll_result",
                "beads poll completed"
            );
            app.on_poll_result(tasks);
        }
        AppEvent::PollFailed(error) => {
            warn!(
                %error,
                consecutive_failures = app.consecutive_poll_failures + 1,
                event = "poll_failed",
                "beads poll failed"
            );
            app.on_poll_failed(error);
        }
        AppEvent::AgentOutput { agent_id, line } => {
            app.on_agent_output(agent_id, line);
        }
        AppEvent::AgentExited {
            agent_id,
            exit_code,
        } => {
            // Look up agent metadata before processing the exit
            let (task_id, runtime, model, elapsed, retry_count) = app
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| (
                    a.task.id.clone(),
                    a.runtime.name().to_string(),
                    a.model.clone(),
                    a.started_at.elapsed().as_secs(),
                    a.retry_count,
                ))
                .unwrap_or_default();
            let success = exit_code == Some(0);
            if success {
                info!(
                    agent_id,
                    task_id,
                    runtime,
                    model,
                    elapsed_secs = elapsed,
                    retry_count,
                    exit_code = 0,
                    event = "agent_completed",
                    "agent completed successfully"
                );
            } else {
                warn!(
                    agent_id,
                    task_id,
                    runtime,
                    model,
                    elapsed_secs = elapsed,
                    retry_count,
                    ?exit_code,
                    event = "agent_failed",
                    "agent exited with failure"
                );
            }
            app.on_agent_exited(agent_id, exit_code);
            // When an agent completes successfully, a child issue was likely closed —
            // check if any parent epics are now eligible for auto-closure.
            if success {
                let tx_epic = tx.clone();
                tokio::spawn(async move {
                    let result = runtime::close_eligible_epics().await;
                    let _ = tx_epic.send(AppEvent::EpicCloseResult(result));
                });
            }
        }
        AppEvent::AgentPid { agent_id, pid } => {
            debug!(agent_id, pid, event = "agent_pid", "agent process started");
            app.on_agent_pid(agent_id, pid);
        }
        AppEvent::AgentPtyData { agent_id, data } => {
            app.on_agent_pty_data(agent_id, &data);
        }
        AppEvent::AgentPtyReady { agent_id, handle } => {
            debug!(agent_id, event = "pty_ready", "agent PTY handle ready");
            app.on_agent_pty_ready(agent_id, *handle);
        }
        AppEvent::WorktreeOrphans(paths) => {
            warn!(
                count = paths.len(),
                event = "worktree_orphans",
                "orphaned agent worktrees found from previous session"
            );
            app.on_worktree_orphans(paths);
        }
        AppEvent::WorktreeCleaned { cleaned, failed } => {
            if !cleaned.is_empty() || !failed.is_empty() {
                info!(
                    cleaned = cleaned.len(),
                    failed = failed.len(),
                    event = "worktree_cleaned",
                    "worktree cleanup completed"
                );
            }
            app.on_worktree_cleaned(cleaned, failed);
        }
        AppEvent::DiffResult { agent_id, diff } => {
            app.on_diff_result(agent_id, diff);
        }
        AppEvent::WorktreeScanned(worktrees) => {
            app.on_worktree_scanned(worktrees);
        }
        AppEvent::DepGraphResult(nodes) => {
            app.on_dep_graph_result(nodes);
        }
        AppEvent::DepGraphFailed(error) => {
            warn!(%error, event = "dep_graph_failed", "dependency graph poll failed");
            app.on_dep_graph_failed(error);
        }
        AppEvent::BlockedPollResult(tasks) => {
            debug!(
                blocked_count = tasks.len(),
                event = "blocked_poll_result",
                "blocked issues poll completed"
            );
            app.on_blocked_poll_result(tasks);
        }
        AppEvent::IssueStatusClosed { agent_id } => {
            app.on_issue_closed(agent_id);
            // Child issue was closed — check if any parent epics are now eligible
            let tx_epic = tx.clone();
            tokio::spawn(async move {
                let result = runtime::close_eligible_epics().await;
                let _ = tx_epic.send(AppEvent::EpicCloseResult(result));
            });
        }
        AppEvent::EpicCloseResult(result) => {
            match result {
                Ok(closed_ids) if !closed_ids.is_empty() => {
                    info!(
                        closed_epics = %closed_ids.join(","),
                        event = "epics_auto_closed",
                        "epics auto-closed after all children completed"
                    );
                    app.log(
                        LogCategory::Complete,
                        format!("Epic(s) auto-closed: {}", closed_ids.join(", ")),
                    );
                    app.alert_message = Some((
                        format!("EPIC(S) CLOSED: {}", closed_ids.join(", ")),
                        app.frame_count + 80,
                    ));
                }
                Err(e) => {
                    warn!(%e, event = "epic_close_failed", "failed to auto-close eligible epics");
                }
                _ => {} // No epics were eligible — nothing to do
            }
        }
        AppEvent::IssueCreateResult(result) => {
            match result {
                Ok(issue_id) => {
                    app.log(
                        LogCategory::System,
                        format!("Issue created: {}", issue_id),
                    );
                    app.alert_message = Some((
                        format!("Issue created: {}", issue_id),
                        app.frame_count + 120,
                    ));
                    // Trigger a poll to pick up the new issue
                    app.poll_countdown = 0.0;
                    let tx_poll = tx.clone();
                    let tx_blocked = tx.clone();
                    tokio::spawn(async move {
                        match runtime::poll_ready().await {
                            Ok(tasks) => {
                                let _ = tx_poll.send(AppEvent::PollResult(tasks));
                            }
                            Err(e) => {
                                let _ = tx_poll.send(AppEvent::PollFailed(e.to_string()));
                            }
                        }
                        if let Ok(blocked) = runtime::poll_blocked().await {
                            let _ = tx_blocked.send(AppEvent::BlockedPollResult(blocked));
                        }
                    });
                }
                Err(e) => {
                    app.log(
                        LogCategory::Alert,
                        format!("Issue creation failed: {}", e),
                    );
                    app.alert_message = Some((
                        format!("Create failed: {}", e),
                        app.frame_count + 180,
                    ));
                }
            }
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

    // ── Interactive mode: forward everything to PTY except F2 (detach) ──
    if app.interactive_mode {
        // F2 = detach from interactive mode
        if key.code == KeyCode::F(2) {
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

    // ── Quit confirmation dialog: intercept y/n/Esc ──
    if app.confirm_quit {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                app.should_quit = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.confirm_quit = false;
            }
            _ => {}
        }
        return;
    }

    // ── Mark-complete confirmation dialog: intercept y/n/Esc ──
    if app.confirm_complete_agent_id.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(agent_id) = app.confirm_complete_agent_id.take() {
                    if let Some((_, Some(worktree_path))) = app.force_complete_agent(agent_id) {
                        let branch = std::path::Path::new(&worktree_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.strip_prefix("worktree-"))
                            .unwrap_or("")
                            .to_string();
                        let tx_done = tx.clone();
                        tokio::spawn(async move {
                            let mut cleaned = Vec::new();
                            let mut failed = Vec::new();
                            match runtime::cleanup_worktree(&worktree_path, &branch).await {
                                Ok(()) => cleaned.push(worktree_path),
                                Err(_) => failed.push(worktree_path),
                            }
                            let _ = tx_done.send(AppEvent::WorktreeCleaned { cleaned, failed });
                        });
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.confirm_complete_agent_id = None;
            }
            _ => {}
        }
        return;
    }

    // ── Kill confirmation dialog: intercept y/n/Esc ──
    if app.confirm_kill_agent_id.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(agent_id) = app.confirm_kill_agent_id.take() {
                    if let Some((_, Some(worktree_path))) = app.kill_agent(agent_id) {
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
            KeyCode::Char('n') | KeyCode::Esc => {
                app.confirm_kill_agent_id = None;
            }
            _ => {}
        }
        return;
    }

    // ── Issue creation form: intercept keys when form is active ──
    if app.issue_creation_active {
        match key.code {
            KeyCode::Esc => {
                app.issue_creation_active = false;
                app.issue_creation_form = types::IssueCreationForm::new();
            }
            KeyCode::Tab => {
                app.issue_creation_form.focused_field =
                    (app.issue_creation_form.focused_field + 1) % 4;
            }
            KeyCode::BackTab => {
                app.issue_creation_form.focused_field =
                    (app.issue_creation_form.focused_field + 3) % 4;
            }
            KeyCode::Enter => {
                let form = &app.issue_creation_form;
                if form.title.trim().is_empty() {
                    app.alert_message = Some((
                        "Title is required".to_string(),
                        app.frame_count + 60,
                    ));
                } else {
                    let title = form.title.clone();
                    let description = form.description.clone();
                    let issue_type = form.issue_type().to_string();
                    let priority = form.priority;
                    let tx_create = tx.clone();
                    tokio::spawn(async move {
                        let result =
                            runtime::create_issue(&title, &description, &issue_type, priority)
                                .await;
                        let _ = tx_create.send(AppEvent::IssueCreateResult(result));
                    });
                    app.issue_creation_active = false;
                    app.issue_creation_form = types::IssueCreationForm::new();
                    app.log(LogCategory::System, "Creating issue...".into());
                }
            }
            KeyCode::Up => {
                match app.issue_creation_form.focused_field {
                    2 => {
                        // Cycle issue type backward
                        let len = types::ISSUE_TYPES.len();
                        app.issue_creation_form.issue_type_idx =
                            (app.issue_creation_form.issue_type_idx + len - 1) % len;
                    }
                    3 => {
                        // Increase priority (lower number = higher priority)
                        app.issue_creation_form.priority =
                            (app.issue_creation_form.priority - 1).max(1);
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match app.issue_creation_form.focused_field {
                    2 => {
                        // Cycle issue type forward
                        app.issue_creation_form.issue_type_idx =
                            (app.issue_creation_form.issue_type_idx + 1) % types::ISSUE_TYPES.len();
                    }
                    3 => {
                        // Decrease priority (higher number = lower priority)
                        app.issue_creation_form.priority =
                            (app.issue_creation_form.priority + 1).min(4);
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                match app.issue_creation_form.focused_field {
                    0 => { app.issue_creation_form.title.pop(); }
                    1 => { app.issue_creation_form.description.pop(); }
                    _ => {}
                }
            }
            KeyCode::Char(c) => {
                match app.issue_creation_form.focused_field {
                    0 => app.issue_creation_form.title.push(c),
                    1 => app.issue_creation_form.description.push(c),
                    _ => {}
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        // Event log category filter cycling
        KeyCode::Char('f') if app.active_view == View::EventLog => {
            app.cycle_log_category_filter();
        }
        // Worktree overview sort toggle
        KeyCode::Char('f') if app.active_view == View::WorktreeOverview => {
            app.cycle_worktree_sort();
        }
        // ── Dep graph: Enter toggles collapse/expand ──
        KeyCode::Enter if app.active_view == View::DepGraph => {
            app.dep_graph_toggle_collapse();
        }
        KeyCode::Char('q') => {
            if app.active_view == View::AgentDetail || app.active_view == View::SplitPane || app.active_view == View::WorktreeOverview || app.active_view == View::DepGraph {
                app.interactive_mode = false;
                app.search_active = false;
                app.search_query.clear();
                app.search_matches.clear();
                app.active_view = View::Dashboard;
            } else if app.active_agent_count() > 0 {
                app.confirm_quit = true;
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
        KeyCode::Char('3') => {
            app.active_view = View::EventLog;
            app.event_log_seen_count = app.event_log.len();
        }
        KeyCode::Char('4') => app.active_view = View::History,
        KeyCode::Char('5') => {
            app.auto_fill_split_panes();
            app.active_view = View::SplitPane;
        }
        KeyCode::Char('6') => {
            app.active_view = View::WorktreeOverview;
            // Trigger immediate scan
            app.worktree_last_scan_frame = 0;
        }
        KeyCode::Char('7') => {
            app.active_view = View::DepGraph;
            // Trigger immediate poll
            app.dep_graph_last_poll_frame = 0;
        }
        KeyCode::Char('w') if app.active_view == View::Dashboard => {
            app.active_view = View::WorktreeOverview;
            // Trigger immediate scan
            app.worktree_last_scan_frame = 0;
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
                            format!("Attached to AGENT-{:02} — F2 to detach", agent_id),
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
                // Only show confirmation if the agent is actually killable
                let is_killable = app.agents.iter().any(|a| {
                    a.id == agent_id
                        && matches!(a.status, crate::types::AgentStatus::Starting | crate::types::AgentStatus::Running)
                });
                if is_killable {
                    app.confirm_kill_agent_id = Some(agent_id);
                }
            }
        }
        KeyCode::Char('D') if app.active_view == View::AgentDetail => {
            if let Some(agent_id) = app.selected_agent_id {
                let is_active = app.agents.iter().any(|a| {
                    a.id == agent_id
                        && matches!(a.status, crate::types::AgentStatus::Starting | crate::types::AgentStatus::Running)
                });
                if is_active {
                    app.confirm_complete_agent_id = Some(agent_id);
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
            let tx_blocked = tx.clone();
            tokio::spawn(async move {
                match runtime::poll_ready().await {
                    Ok(tasks) => {
                        let _ = tx.send(AppEvent::PollResult(tasks));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::PollFailed(e.to_string()));
                    }
                }
                if let Ok(blocked) = runtime::poll_blocked().await {
                    let _ = tx_blocked.send(AppEvent::BlockedPollResult(blocked));
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
        // 'C' opens the issue creation form from Dashboard
        KeyCode::Char('C') if app.active_view == View::Dashboard => {
            app.issue_creation_active = true;
            app.issue_creation_form = types::IssueCreationForm::new();
        }
        // 'd' toggles diff panel in Agent Detail view
        KeyCode::Char('d') if app.active_view == View::AgentDetail => {
            app.toggle_diff_panel();
            if app.show_diff_panel {
                // Fire immediate diff poll
                if let Some(wt_path) = app.selected_agent_worktree() {
                    if let Some(agent_id) = app.selected_agent_id {
                        let tx_diff = tx.clone();
                        tokio::spawn(async move {
                            let diff = runtime::poll_worktree_diff(&wt_path).await;
                            let _ = tx_diff.send(AppEvent::DiffResult { agent_id, diff });
                        });
                    }
                }
            }
        }
        // Export agent log to file
        KeyCode::Char('e') if app.active_view == View::AgentDetail => {
            match app.export_agent_log() {
                Ok(path) => {
                    app.alert_message = Some((format!("Log exported: {}", path), app.frame_count + 180));
                }
                Err(msg) => {
                    app.alert_message = Some((format!("Export failed: {}", msg), app.frame_count + 180));
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
        // ── Yank (copy to clipboard) ──
        KeyCode::Char('y')
            if matches!(
                app.active_view,
                View::Dashboard | View::AgentDetail
            ) =>
        {
            if let Some(text) = app.yank_text() {
                match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
                    Ok(()) => {
                        app.alert_message = Some((
                            format!("Copied: {}", text),
                            app.frame_count + 30, // ~3 seconds
                        ));
                    }
                    Err(e) => {
                        app.log(
                            LogCategory::System,
                            format!("Clipboard error: {}", e),
                        );
                    }
                }
            }
        }
        _ => {}
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

    let task_id = task.id.clone();
    let task_title = task.title.clone();
    let runtime_name = runtime.to_string();
    let model_name = model.clone();
    info!(
        agent_id,
        task_id,
        task_title,
        runtime = %runtime_name,
        model = %model_name,
        pty_rows,
        pty_cols,
        event = "agent_spawn",
        "spawning agent process"
    );

    // PTY creation is blocking — run in spawn_blocking
    let spawn_result = tokio::task::spawn_blocking(move || {
        runtime::spawn_agent_pty(runtime, &model, &task, &system_prompt, &user_prompt, pty_rows, pty_cols)
    })
    .await;

    let (handle, reader, child) = match spawn_result {
        Ok(Ok(tuple)) => tuple,
        Ok(Err(e)) => {
            error!(
                agent_id,
                task_id,
                runtime = %runtime_name,
                %e,
                event = "pty_spawn_failed",
                "PTY spawn failed"
            );
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
            error!(
                agent_id,
                task_id,
                runtime = %runtime_name,
                %e,
                event = "pty_spawn_panic",
                "spawn_blocking panicked during PTY creation"
            );
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
        handle: Box::new(handle),
    });

    // Reader task: read raw bytes from PTY, send as events
    let tx_reader = tx.clone();
    let reader_task_id = task_id.clone();
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
                Err(e) => {
                    warn!(
                        agent_id,
                        task_id = %reader_task_id,
                        error = %e,
                        event = "pty_read_error",
                        "PTY reader encountered an error"
                    );
                    break;
                }
            }
        }
    });

    // Exit watcher
    let tx_exit = tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut child = child;
        let exit_code = match child.wait() {
            Ok(status) => {
                if status.success() {
                    Some(0)
                } else {
                    // portable_pty::ExitStatus doesn't expose the raw code,
                    // so we only know it was non-zero.
                    Some(1)
                }
            }
            Err(e) => {
                warn!(
                    agent_id,
                    task_id,
                    error = %e,
                    event = "agent_wait_error",
                    "child.wait() failed — exit code unknown"
                );
                None
            }
        };
        let _ = tx_exit.send(AppEvent::AgentExited {
            agent_id,
            exit_code,
        });
    });
}
