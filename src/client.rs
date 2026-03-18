//! CLI client that sends commands to a running obelisk daemon via TCP.

use crate::daemon::{DaemonCmd, DaemonResp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Connect to the daemon, send a command, and return the response.
async fn send_cmd(cmd: &DaemonCmd) -> Result<DaemonResp, String> {
    let port = crate::daemon::read_daemon_port()?;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .map_err(|e| format!("failed to connect to daemon: {}", e))?;

    let payload =
        serde_json::to_vec(&cmd).map_err(|e| format!("failed to serialize command: {}", e))?;
    stream
        .write_all(&payload)
        .await
        .map_err(|e| format!("failed to send command: {}", e))?;
    stream
        .shutdown()
        .await
        .map_err(|e| format!("failed to shutdown write: {}", e))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("failed to read response: {}", e))?;

    serde_json::from_slice(&buf).map_err(|e| format!("failed to parse response: {}", e))
}

/// Run a CLI subcommand against the daemon.
pub async fn run(cmd: ClientCommand) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_cmd = match &cmd {
        ClientCommand::Status => DaemonCmd::Status,
        ClientCommand::Agents => DaemonCmd::Agents,
        ClientCommand::Spawn { issue_id } => DaemonCmd::Spawn {
            issue_id: issue_id.clone(),
        },
        ClientCommand::Kill { agent_id } => DaemonCmd::Kill {
            agent_id: *agent_id,
        },
        ClientCommand::Stop => DaemonCmd::Stop,
    };

    match send_cmd(&daemon_cmd).await {
        Ok(resp) => {
            if let Some(msg) = &resp.message {
                if resp.ok {
                    println!("{}", msg);
                } else {
                    eprintln!("error: {}", msg);
                    std::process::exit(1);
                }
            }
            if let Some(data) = &resp.data {
                match &cmd {
                    ClientCommand::Status => print_status(data),
                    ClientCommand::Agents => print_agents(data),
                    _ => {
                        println!("{}", serde_json::to_string_pretty(data)
                            .unwrap_or_else(|_| format!("{:?}", data)));
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_status(data: &serde_json::Value) {
    println!("obelisk daemon status");
    println!("  active agents:  {}/{}", data["active_agents"], data["max_concurrent"]);
    println!("  ready tasks:    {}", data["ready_tasks"]);
    println!("  completed:      {}", data["total_completed"]);
    println!("  failed:         {}", data["total_failed"]);
    println!("  auto-spawn:     {}", data["auto_spawn"]);
    println!("  runtime:        {}", data["runtime"].as_str().unwrap_or("?"));
    println!("  total agents:   {}", data["agents_total"]);
    println!("  session:        {}", data["session_id"].as_str().unwrap_or("?"));
}

fn print_agents(data: &serde_json::Value) {
    let agents = match data["agents"].as_array() {
        Some(a) => a,
        None => {
            println!("no agents");
            return;
        }
    };

    if agents.is_empty() {
        println!("no agents");
        return;
    }

    // Header
    println!(
        "{:<4} {:<18} {:<10} {:<14} {:<10} {:<8}",
        "ID", "TASK", "STATUS", "PHASE", "RUNTIME", "ELAPSED"
    );
    println!("{}", "-".repeat(68));

    for a in agents {
        let elapsed = a["elapsed_secs"].as_u64().unwrap_or(0);
        let mins = elapsed / 60;
        let secs = elapsed % 60;

        println!(
            "{:<4} {:<18} {:<10} {:<14} {:<10} {:>5}:{:02}",
            a["id"],
            a["task_id"].as_str().unwrap_or("?"),
            a["status_label"].as_str().unwrap_or("?"),
            a["phase"].as_str().unwrap_or("?"),
            a["runtime"].as_str().unwrap_or("?"),
            mins,
            secs,
        );
    }
}

/// Parsed CLI subcommand for client mode.
#[derive(Debug)]
pub enum ClientCommand {
    Status,
    Agents,
    Spawn { issue_id: String },
    Kill { agent_id: usize },
    Stop,
}
