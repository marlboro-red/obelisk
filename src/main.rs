mod app;
mod runtime;
mod types;
mod ui;

use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;
use types::{AppEvent, LogCategory, View};

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

    // Main loop
    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        match rx.recv().await {
            Some(AppEvent::Terminal(Event::Key(key))) => {
                handle_key(&mut app, key, &tx);
            }
            Some(AppEvent::Tick) => {
                app.on_tick();

                if app.auto_spawn {
                    while let Some(req) = app.get_auto_spawn_info() {
                        tokio::spawn(spawn_agent_process(tx.clone(), req));
                    }
                }
            }
            Some(AppEvent::PollResult(tasks)) => {
                app.on_poll_result(tasks);
            }
            Some(AppEvent::AgentOutput { agent_id, line }) => {
                app.on_agent_output(agent_id, line);
            }
            Some(AppEvent::AgentExited { agent_id, exit_code }) => {
                app.on_agent_exited(agent_id, exit_code);
            }
            Some(AppEvent::AgentPid { agent_id, pid }) => {
                app.on_agent_pid(agent_id, pid);
            }
            Some(AppEvent::Terminal(_)) => {}
            None => break,
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_key(
    app: &mut App,
    key: event::KeyEvent,
    tx: &mpsc::UnboundedSender<AppEvent>,
) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match key.code {
        KeyCode::Char('q') => {
            if app.active_view == View::AgentDetail {
                app.active_view = View::Dashboard;
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Esc => {
            if app.active_view != View::Dashboard {
                app.active_view = View::Dashboard;
            }
        }
        KeyCode::Char('1') => app.active_view = View::Dashboard,
        KeyCode::Char('2') => {
            if app.selected_agent_id.is_none() && !app.agents.is_empty() {
                app.selected_agent_id = Some(app.agents[0].id);
            }
            app.active_view = View::AgentDetail;
        }
        KeyCode::Char('3') => app.active_view = View::EventLog,
        KeyCode::Up => app.navigate_up(),
        KeyCode::Down => app.navigate_down(),
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
                format!("Runtime switched to {}", app.selected_runtime.name()),
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
        KeyCode::Left if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i > 0 {
                        app.selected_agent_id = Some(app.agents[i - 1].id);
                        app.agent_output_scroll = 0;
                    }
                }
            }
        }
        KeyCode::Right if app.active_view == View::AgentDetail => {
            if let Some(current_id) = app.selected_agent_id {
                if let Some(i) = app.agents.iter().position(|a| a.id == current_id) {
                    if i + 1 < app.agents.len() {
                        app.selected_agent_id = Some(app.agents[i + 1].id);
                        app.agent_output_scroll = 0;
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

async fn spawn_agent_process(
    tx: mpsc::UnboundedSender<AppEvent>,
    req: app::SpawnRequest,
) {
    let agent_id = req.agent_id;
    // Agent prompt handles claim/close lifecycle itself
    match runtime::spawn_agent(req.runtime, &req.task, &req.system_prompt, &req.user_prompt).await
    {
        Ok(mut spawned) => {
            // Record PID for kill support
            if let Some(pid) = spawned.child.id() {
                let _ = tx.send(AppEvent::AgentPid { agent_id, pid });
            }

            // Read stdout
            let tx_stdout = tx.clone();
            tokio::spawn(async move {
                while let Ok(Some(line)) = spawned.stdout.next_line().await {
                    if tx_stdout
                        .send(AppEvent::AgentOutput {
                            agent_id,
                            line,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });

            // Read stderr
            let tx_stderr = tx.clone();
            tokio::spawn(async move {
                while let Ok(Some(line)) = spawned.stderr.next_line().await {
                    if tx_stderr
                        .send(AppEvent::AgentOutput {
                            agent_id,
                            line: format!("[stderr] {}", line),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });

            // Wait for exit
            let tx_exit = tx.clone();
            tokio::spawn(async move {
                let exit_status = spawned.child.wait().await;
                let exit_code = exit_status.ok().and_then(|s| s.code());

                let _ = tx_exit.send(AppEvent::AgentExited {
                    agent_id,
                    exit_code,
                });
            });
        }
        Err(e) => {
            let _ = tx.send(AppEvent::AgentOutput {
                agent_id,
                line: format!("[ERROR] {}", e),
            });
            let _ = tx.send(AppEvent::AgentExited {
                agent_id,
                exit_code: Some(1),
            });
        }
    }
}
