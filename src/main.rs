mod app;
mod runtime;
mod types;
mod ui;

use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
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
                Err(_e) => {
                    let _ = tx_poll.send(AppEvent::PollResult(Vec::new()));
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

    // Initial render before entering event loop
    let mut prev_view = app.active_view;
    let mut prev_term_size = terminal.size()?;
    terminal.draw(|f| ui::render(f, &app))?;

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

            terminal.draw(|f| ui::render(f, &app))?;
        }
    }

    // Kill all running agent processes before exiting
    app.kill_all_agents();

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
        AppEvent::Tick => {
            app.on_tick();

            if app.auto_spawn {
                while let Some(req) = app.get_auto_spawn_info() {
                    tokio::spawn(spawn_agent_process(tx.clone(), req));
                }
            }
        }
        AppEvent::PollResult(tasks) => {
            app.on_poll_result(tasks);
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

    match key.code {
        KeyCode::Char('q') => {
            if app.active_view == View::AgentDetail {
                app.interactive_mode = false;
                app.active_view = View::Dashboard;
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Esc => {
            if app.active_view != View::Dashboard {
                app.interactive_mode = false;
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

        KeyCode::Up => app.navigate_up(),
        KeyCode::Down => app.navigate_down(),
        KeyCode::PageUp if app.active_view == View::AgentDetail => app.page_up(),
        KeyCode::PageDown if app.active_view == View::AgentDetail => app.page_down(),
        KeyCode::Home if app.active_view == View::AgentDetail => {
            app.agent_output_scroll = Some(0);
        }
        KeyCode::End if app.active_view == View::AgentDetail => {
            app.agent_output_scroll = None; // re-engage auto-follow
        }
        KeyCode::Char('k') if app.active_view == View::AgentDetail => {
            if let Some(agent_id) = app.selected_agent_id {
                app.kill_agent(agent_id);
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
                    Err(_) => {
                        let _ = tx.send(AppEvent::PollResult(Vec::new()));
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
        KeyCode::Left if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i > 0 {
                        app.interactive_mode = false;
                        app.selected_agent_id = Some(app.agents[i - 1].id);
                        app.agent_output_scroll = None;
                    }
                }
            }
        }
        KeyCode::Right if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i + 1 < app.agents.len() {
                        app.interactive_mode = false;
                        app.selected_agent_id = Some(app.agents[i + 1].id);
                        app.agent_output_scroll = None;
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
