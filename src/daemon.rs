//! Headless daemon mode — runs the orchestrator without the TUI.
//!
//! Listens on a TCP socket (`127.0.0.1`, random port stored in `.beads/obelisk.port`)
//! for JSON commands from CLI clients. Manages agent spawning, polling, and lifecycle
//! identically to the TUI mode but logs to stdout/file instead of rendering.

use crate::app::{App, SpawnRequest};
use crate::runtime;
use crate::types::{AgentStatus, AppEvent, LogCategory};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

const PORT_FILE: &str = ".beads/obelisk.port";

/// JSON command sent by CLI clients over the socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum DaemonCmd {
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "agents")]
    Agents,
    #[serde(rename = "spawn")]
    Spawn { issue_id: String },
    #[serde(rename = "kill")]
    Kill { agent_id: usize },
    #[serde(rename = "stop")]
    Stop,
}

/// JSON response returned to CLI clients.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResp {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Path to the port file used by the daemon for client discovery.
pub fn port_file_path() -> PathBuf {
    PathBuf::from(PORT_FILE)
}

/// Read the daemon's TCP port from the port file.
pub fn read_daemon_port() -> Result<u16, String> {
    let path = port_file_path();
    let contents = std::fs::read_to_string(&path)
        .map_err(|_| "daemon is not running (port file not found). Start it with: obelisk serve".to_string())?;
    contents.trim().parse::<u16>()
        .map_err(|e| format!("invalid port in {}: {}", path.display(), e))
}

/// Run the daemon event loop. Blocks until a `stop` command is received or the
/// process is signalled.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let port_file = port_file_path();
    // Clean up stale port file
    if port_file.exists() {
        std::fs::remove_file(&port_file)?;
    }
    // Ensure parent directory exists
    if let Some(parent) = port_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    std::fs::write(&port_file, port.to_string())?;
    info!("daemon listening on 127.0.0.1:{}", port);
    eprintln!("obelisk daemon listening on 127.0.0.1:{}", port);

    let mut app = App::new();
    // Daemon uses a fixed PTY size (no terminal to measure)
    app.last_pty_size = (40, 120);

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Shared poll interval — updated by config hot-reload, read by poller
    let shared_poll_interval = Arc::new(AtomicU64::new(app.poll_interval_secs));

    // Poller task
    let tx_poll = tx.clone();
    let poller_interval = Arc::clone(&shared_poll_interval);
    tokio::spawn(async move {
        let mut cycle = 0u64;
        loop {
            match runtime::poll_ready().await {
                Ok(tasks) => {
                    if tx_poll.send(AppEvent::PollResult(tasks)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx_poll.send(AppEvent::PollFailed(e));
                }
            }
            // Poll blocked issues on every cycle (needed for dep-aware auto-spawn)
            if let Ok(blocked) = runtime::poll_blocked().await {
                let _ = tx_poll.send(AppEvent::BlockedPollResult(blocked));
            }
            // Poll dep graph every 3rd cycle for dependency-aware auto-spawn
            if cycle % 3 == 0 {
                match runtime::poll_dep_graph().await {
                    Ok(nodes) => {
                        let _ = tx_poll.send(AppEvent::DepGraphResult(nodes));
                    }
                    Err(e) => {
                        let _ = tx_poll.send(AppEvent::DepGraphFailed(e));
                    }
                }
            }
            cycle += 1;
            let secs = poller_interval.load(Ordering::Relaxed);
            tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
        }
    });

    // Tick timer (slower than TUI — 1s ticks are fine for headless)
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tx_tick.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });

    // Socket accept loop runs in its own task, forwarding commands via a channel.
    let (cmd_tx, mut cmd_rx) =
        mpsc::unbounded_channel::<(DaemonCmd, tokio::sync::oneshot::Sender<DaemonResp>)>();

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut stream, _)) => {
                    let cmd_tx = cmd_tx.clone();
                    tokio::spawn(async move {
                        let mut buf = vec![0u8; 8192];
                        let n = match stream.read(&mut buf).await {
                            Ok(n) => n,
                            Err(_) => return,
                        };
                        let cmd: DaemonCmd = match serde_json::from_slice(&buf[..n]) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp = DaemonResp {
                                    ok: false,
                                    message: Some(format!("invalid command: {}", e)),
                                    data: None,
                                };
                                if let Ok(json) = serde_json::to_string(&resp) {
                                    let _ = stream.write_all(json.as_bytes()).await;
                                }
                                return;
                            }
                        };
                        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                        if cmd_tx.send((cmd, resp_tx)).is_err() {
                            return;
                        }
                        if let Ok(resp) = resp_rx.await {
                            if let Ok(json) = serde_json::to_string(&resp) {
                                let _ = stream.write_all(json.as_bytes()).await;
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("socket accept error: {}", e);
                }
            }
        }
    });

    // Main event loop
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(ev) => process_daemon_event(&mut app, ev, &tx, &shared_poll_interval),
                    None => break,
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some((daemon_cmd, resp_tx)) => {
                        let (resp, should_stop) = handle_daemon_cmd(&mut app, daemon_cmd, &tx);
                        let _ = resp_tx.send(resp);
                        if should_stop {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    // Shutdown
    info!("daemon shutting down");
    eprintln!("obelisk daemon shutting down");
    app.save_config();
    app.kill_all_agents();
    app.save_session();

    // Clean up port file
    let _ = std::fs::remove_file(&port_file);

    Ok(())
}

/// Process internal events (agent output, poll results, ticks).
fn process_daemon_event(
    app: &mut App,
    event: AppEvent,
    tx: &mpsc::UnboundedSender<AppEvent>,
    shared_poll_interval: &Arc<AtomicU64>,
) {
    match event {
        AppEvent::Tick => {
            app.on_tick();

            // Config hot-reload check (~every 2s)
            if app.check_config_reload() {
                shared_poll_interval.store(app.poll_interval_secs, Ordering::Relaxed);
            }

            // Auto-spawn if enabled
            if app.auto_spawn {
                while let Some(req) = app.get_auto_spawn_info() {
                    let unit = req.agent_id;
                    let task_id = req.task.id.clone();
                    info!(agent_id = unit, task_id, event = "auto_spawn", "auto-spawning agent for ready task");
                    tokio::spawn(spawn_agent_process(tx.clone(), req));
                }
            }

            // Periodic issue status poll (~every 5s = 5 ticks at 1s daemon tick rate)
            if app.frame_count.is_multiple_of(5) {
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
        AppEvent::AgentExited { agent_id, exit_code } => {
            let (task_id, runtime, elapsed) = app
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| (
                    a.task.id.clone(),
                    a.runtime.name().to_string(),
                    a.started_at.elapsed().as_secs(),
                ))
                .unwrap_or_default();
            if exit_code == Some(0) {
                info!(
                    agent_id,
                    task_id,
                    runtime,
                    elapsed_secs = elapsed,
                    exit_code = 0,
                    event = "agent_completed",
                    "agent completed successfully"
                );
            } else {
                warn!(
                    agent_id,
                    task_id,
                    runtime,
                    elapsed_secs = elapsed,
                    ?exit_code,
                    event = "agent_failed",
                    "agent exited with failure"
                );
            }
            app.on_agent_exited(agent_id, exit_code);
            if exit_code == Some(0) {
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
                }
                Err(e) => {
                    warn!(%e, event = "epic_close_failed", "failed to auto-close eligible epics");
                }
                _ => {}
            }
        }
        AppEvent::Terminal(_) => {}  // no TUI in daemon mode
        AppEvent::IssueCreateResult(_) => {} // no TUI in daemon mode
    }
}

/// Handle a command from a CLI client. Returns (response, should_stop).
fn handle_daemon_cmd(
    app: &mut App,
    cmd: DaemonCmd,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> (DaemonResp, bool) {
    match cmd {
        DaemonCmd::Status => {
            let data = serde_json::json!({
                "running": true,
                "active_agents": app.active_agent_count(),
                "max_concurrent": app.max_concurrent,
                "total_completed": app.total_completed,
                "total_failed": app.total_failed,
                "auto_spawn": app.auto_spawn,
                "runtime": app.selected_runtime.name(),
                "ready_tasks": app.ready_tasks.len(),
                "agents_total": app.agents.len(),
                "session_id": app.session_id,
            });
            (DaemonResp { ok: true, message: None, data: Some(data) }, false)
        }
        DaemonCmd::Agents => {
            let agents: Vec<serde_json::Value> = app
                .agents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id,
                        "task_id": a.task.id,
                        "title": a.task.title,
                        "status": format!("{}", a.status.symbol()),
                        "status_label": match a.status {
                            AgentStatus::Starting => "starting",
                            AgentStatus::Running => "running",
                            AgentStatus::Completed => "completed",
                            AgentStatus::Failed => "failed",
                        },
                        "phase": a.phase.label(),
                        "runtime": a.runtime.name(),
                        "model": a.model,
                        "elapsed_secs": a.elapsed_secs,
                        "pid": a.pid,
                    })
                })
                .collect();
            let data = serde_json::json!({ "agents": agents });
            (DaemonResp { ok: true, message: None, data: Some(data) }, false)
        }
        DaemonCmd::Spawn { issue_id } => {
            match spawn_task_by_id(app, &issue_id, tx) {
                Ok(agent_id) => (
                    DaemonResp {
                        ok: true,
                        message: Some(format!("spawned agent #{} for {}", agent_id, issue_id)),
                        data: Some(serde_json::json!({ "agent_id": agent_id })),
                    },
                    false,
                ),
                Err(e) => (
                    DaemonResp { ok: false, message: Some(e), data: None },
                    false,
                ),
            }
        }
        DaemonCmd::Kill { agent_id } => {
            match app.kill_agent(agent_id) {
                Some((unit, _)) => (
                    DaemonResp {
                        ok: true,
                        message: Some(format!("killed agent #{}", unit)),
                        data: None,
                    },
                    false,
                ),
                None => (
                    DaemonResp {
                        ok: false,
                        message: Some(format!("agent {} not found or not running", agent_id)),
                        data: None,
                    },
                    false,
                ),
            }
        }
        DaemonCmd::Stop => (
            DaemonResp { ok: true, message: Some("daemon stopping".into()), data: None },
            true,
        ),
    }
}

/// Spawn an agent for a specific issue ID. Looks up the task in the ready queue,
/// creates the agent instance, and kicks off the PTY process.
fn spawn_task_by_id(
    app: &mut App,
    issue_id: &str,
    tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<usize, String> {
    if app.active_agent_count() >= app.max_concurrent {
        return Err("max concurrent agents reached".into());
    }

    // Find task in ready queue
    let task_idx = app
        .ready_tasks
        .iter()
        .position(|t| t.id == issue_id)
        .ok_or_else(|| format!("issue '{}' not found in ready queue", issue_id))?;

    // Select it and use get_spawn_info
    app.task_list_state.select(Some(task_idx));
    let req = app
        .get_spawn_info()
        .ok_or_else(|| "failed to create spawn request".to_string())?;

    let agent_id = req.agent_id;
    tokio::spawn(spawn_agent_process(tx.clone(), req));
    Ok(agent_id)
}

/// Spawn an agent process in a PTY — identical to the TUI version.
async fn spawn_agent_process(
    tx: mpsc::UnboundedSender<AppEvent>,
    req: SpawnRequest,
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
            let _ = tx.send(AppEvent::AgentExited { agent_id, exit_code: Some(1) });
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
            let _ = tx.send(AppEvent::AgentExited { agent_id, exit_code: Some(1) });
            return;
        }
    };

    if let Some(pid) = child.process_id() {
        debug!(agent_id, pid, event = "agent_pid", "agent process started");
        let _ = tx.send(AppEvent::AgentPid { agent_id, pid });
    }

    let _ = tx.send(AppEvent::AgentPtyReady { agent_id, handle: Box::new(handle) });

    // Reader task
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
                if status.success() { Some(0) } else { Some(1) }
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
        let _ = tx_exit.send(AppEvent::AgentExited { agent_id, exit_code });
    });
}
