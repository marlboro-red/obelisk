use crate::types::{BeadTask, Runtime};
use std::process::Stdio;
use tokio::process::Command;

pub struct SpawnedAgent {
    pub child: tokio::process::Child,
    pub stdout: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
    pub stderr: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStderr>>,
}

pub fn build_command(
    runtime: Runtime,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Command {
    match runtime {
        Runtime::ClaudeCode => {
            let mut cmd = Command::new("claude");
            cmd.arg("--print")
                .arg(user_prompt)
                .arg("--model")
                .arg(model)
                .arg("--append-system-prompt")
                .arg(system_prompt)
                .arg("--dangerously-skip-permissions");
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            cmd
        }
        Runtime::Codex => {
            let mut cmd = Command::new("codex");
            cmd.arg("--approval-mode")
                .arg("full-auto")
                .arg("--model")
                .arg(model)
                .arg("--instructions")
                .arg(system_prompt)
                .arg(user_prompt);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            cmd
        }
        Runtime::Copilot => {
            let mut cmd = Command::new("copilot");
            let combined = format!("{}\n\n{}", system_prompt, user_prompt);
            cmd.arg("-p")
                .arg(&combined)
                .arg("--model")
                .arg(model)
                .arg("--yolo");
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            cmd
        }
    }
}

pub async fn spawn_agent(
    runtime: Runtime,
    model: &str,
    _task: &BeadTask,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<SpawnedAgent, String> {
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;

    let mut cmd = build_command(runtime, model, system_prompt, user_prompt);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {} agent: {}", runtime.name(), e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    let stdout_lines = BufReader::new(stdout).lines();
    let stderr_lines = BufReader::new(stderr).lines();

    Ok(SpawnedAgent {
        child,
        stdout: stdout_lines,
        stderr: stderr_lines,
    })
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
