use crate::types::{BeadTask, PtyHandle, Runtime};
use portable_pty::{CommandBuilder, PtySize};
use tokio::process::Command;

/// On Windows, npm-installed CLIs are `.cmd` scripts which `CreateProcessW`
/// cannot resolve directly. Wrap them through `cmd /C` so they launch correctly.
#[cfg(windows)]
fn pty_npm_command(name: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new("cmd");
    cmd.arg("/C");
    cmd.arg(name);
    cmd
}

#[cfg(not(windows))]
fn pty_npm_command(name: &str) -> CommandBuilder {
    CommandBuilder::new(name)
}

/// Build a PTY command that mirrors the original CLI invocation.
/// The process runs in a real terminal (PTY) so it gets ANSI colors,
/// progress bars, and the user can attach and type if needed.
///
/// Claude Code runs in interactive mode (no --print) so the session
/// stays open for steering. Codex and Copilot use their one-shot
/// execution modes (exec / -p) since they lack a documented bare
/// interactive REPL — but the PTY still renders their full output
/// and allows input if the process reads stdin.
pub fn build_pty_command(
    runtime: Runtime,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> CommandBuilder {
    match runtime {
        Runtime::ClaudeCode => {
            // `claude "prompt" --model x --dangerously-skip-permissions`
            // No --print → stays interactive after first response
            let mut cmd = CommandBuilder::new("claude");
            cmd.arg(user_prompt);
            cmd.arg("--model");
            cmd.arg(model);
            cmd.arg("--append-system-prompt");
            cmd.arg(system_prompt);
            cmd.arg("--dangerously-skip-permissions");
            cmd
        }
        Runtime::Codex => {
            // Same invocation as before, but in a PTY for full terminal rendering
            let mut cmd = pty_npm_command("codex");
            let combined = format!(
                "{}\n\nFollow the workflow below exactly.\n\n---\n\n{}",
                user_prompt, system_prompt
            );
            cmd.arg("exec");
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
            cmd.arg("-m");
            cmd.arg(model);
            cmd.arg(combined);
            cmd
        }
        Runtime::Copilot => {
            // Same invocation as before, but in a PTY for full terminal rendering
            let mut cmd = pty_npm_command("copilot");
            let combined = format!(
                "{}\n\nFollow the workflow below exactly.\n\n---\n\n{}",
                user_prompt, system_prompt
            );
            cmd.arg("-p");
            cmd.arg(&combined);
            cmd.arg("--model");
            cmd.arg(model);
            cmd.arg("--yolo");
            cmd
        }
    }
}

/// Spawn an agent in a PTY. Returns the PTY handle (master + writer + parser),
/// a reader for async byte streaming, and the child process.
pub fn spawn_agent_pty(
    runtime: Runtime,
    model: &str,
    _task: &BeadTask,
    system_prompt: &str,
    user_prompt: &str,
    pty_rows: u16,
    pty_cols: u16,
) -> anyhow::Result<(
    PtyHandle,
    Box<dyn std::io::Read + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
)> {
    let pty_system = portable_pty::native_pty_system();
    let size = PtySize {
        rows: pty_rows,
        cols: pty_cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size)?;

    let mut cmd = build_pty_command(runtime, model, system_prompt, user_prompt);
    // Spawn from the current working directory so agents see the user's project
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    let child = pair.slave.spawn_command(cmd)?;
    // Drop slave — the child owns it now
    drop(pair.slave);

    let reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let parser = vt100::Parser::new(pty_rows, pty_cols, 10000);

    let handle = PtyHandle {
        master: pair.master,
        writer,
        parser,
    };

    Ok((handle, reader, child))
}

pub async fn poll_ready() -> Result<Vec<crate::types::BeadTask>, String> {
    let output = Command::new("bd")
        .args(["ready", "--json"])
        .output()
        .await
        .map_err(|e| format!("Failed to run bd ready: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bd ready failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }

    serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed to parse bd ready JSON: {}", e))
}
