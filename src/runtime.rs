use crate::types::{BeadTask, DepNode, DiffData, PtyHandle, Runtime};
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
    let mut writer = pair.master.take_writer()?;

    // ConPTY on Windows sends ESC[6n (Device Status Report) on startup and
    // buffers child output until it receives a cursor-position response.
    // Send the response immediately so output is never stalled.
    use std::io::Write;
    let _ = writer.write_all(b"\x1b[1;1R");
    let _ = writer.flush();

    let parser = vt100::Parser::new(pty_rows, pty_cols, 10000);

    let handle = PtyHandle {
        master: pair.master,
        writer,
        parser,
    };

    Ok((handle, reader, child))
}

/// Scan git worktrees and return those that match the agent worktree naming pattern
/// (`worktree-*`). Returns a list of `(absolute_path, branch_name)` pairs.
pub async fn scan_agent_worktrees() -> Vec<(String, String)> {
    let output = match Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Flush previous entry
            if let (Some(p), Some(b)) = (current_path.take(), current_branch.take()) {
                if is_agent_worktree(&p) {
                    result.push((p, b));
                }
            }
            current_path = Some(path.to_string());
            current_branch = None;
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.to_string());
        }
    }
    // Flush last entry
    if let (Some(p), Some(b)) = (current_path, current_branch) {
        if is_agent_worktree(&p) {
            result.push((p, b));
        }
    }

    result
}

fn is_agent_worktree(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("worktree-"))
        .unwrap_or(false)
}

/// Remove a git worktree at `path` (force) and delete its branch.
/// Returns Ok(()) on success, Err(message) if the worktree removal fails.
/// Branch deletion is best-effort and never causes an error return.
pub async fn cleanup_worktree(path: &str, branch: &str) -> Result<(), String> {
    let status = Command::new("git")
        .args(["worktree", "remove", path, "--force"])
        .status()
        .await
        .map_err(|e| format!("Failed to run git worktree remove: {}", e))?;

    if !status.success() {
        return Err(format!("git worktree remove failed for {}", path));
    }

    // Best-effort branch deletion — ignore errors (branch may not exist yet)
    let _ = Command::new("git")
        .args(["branch", "-D", branch])
        .status()
        .await;

    Ok(())
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

/// Poll all issues with dependencies via `bd dep tree` for each root issue.
/// We use `bd list --json` to find all issues, then `bd dep tree` on each root.
/// For simplicity, we just run `bd list --json` and get all issues, then
/// use `bd dep tree <id> --direction both --json` on the first open/in-progress issue.
pub async fn poll_dep_graph() -> Result<Vec<DepNode>, String> {
    // Get all issues
    let output = Command::new("bd")
        .args(["list", "--json"])
        .output()
        .await
        .map_err(|e| format!("Failed to run bd list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bd list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }

    // Parse as flat list of DepNode (same JSON shape as BeadTask with extra fields)
    let nodes: Vec<DepNode> = serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed to parse bd list JSON: {}", e))?;

    // Now get dependency relationships by running dep tree on each non-closed root
    let mut all_nodes = nodes;

    // Try to enrich with dependency tree data from a root issue
    // Find any issue that has deps (try the first open one)
    if let Some(root) = all_nodes.iter().find(|n| n.status != "closed") {
        let root_id = root.id.clone();
        let dep_output = Command::new("bd")
            .args(["dep", "tree", &root_id, "--direction", "both", "--json"])
            .output()
            .await;

        if let Ok(dep_out) = dep_output {
            if dep_out.status.success() {
                let dep_stdout = String::from_utf8_lossy(&dep_out.stdout);
                let dep_trimmed = dep_stdout.trim();
                if !dep_trimmed.is_empty() && dep_trimmed != "null" {
                    if let Ok(dep_nodes) = serde_json::from_str::<Vec<DepNode>>(dep_trimmed) {
                        // Merge dep tree data — update depth/parent_id for known nodes
                        for dn in &dep_nodes {
                            if let Some(existing) = all_nodes.iter_mut().find(|n| n.id == dn.id) {
                                existing.depth = dn.depth;
                                existing.parent_id = dn.parent_id.clone();
                            } else {
                                all_nodes.push(dn.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all_nodes)
}

/// Run `git diff` + `git diff --cached` in an agent's worktree and parse the result
/// into a structured `DiffData`. Returns an empty diff if the worktree doesn't exist.
pub async fn poll_worktree_diff(worktree_path: &str) -> DiffData {
    let empty = DiffData {
        lines: Vec::new(),
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        changed_files: Vec::new(),
    };

    // Check worktree exists
    if !std::path::Path::new(worktree_path).exists() {
        return empty;
    }

    // Get unstaged diff
    let unstaged = Command::new("git")
        .args(["diff"])
        .current_dir(worktree_path)
        .output()
        .await;

    // Get staged diff
    let staged = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(worktree_path)
        .output()
        .await;

    let mut all_diff = String::new();
    if let Ok(ref out) = unstaged {
        all_diff.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    if let Ok(ref out) = staged {
        let staged_text = String::from_utf8_lossy(&out.stdout);
        if !staged_text.is_empty() {
            if !all_diff.is_empty() {
                all_diff.push('\n');
            }
            all_diff.push_str(&staged_text);
        }
    }

    // Get stat summary
    let stat_output = Command::new("git")
        .args(["diff", "--stat", "--stat-width=200"])
        .current_dir(worktree_path)
        .output()
        .await;

    let stat_cached = Command::new("git")
        .args(["diff", "--cached", "--stat", "--stat-width=200"])
        .current_dir(worktree_path)
        .output()
        .await;

    let mut changed_files = Vec::new();
    let mut insertions = 0usize;
    let mut deletions = 0usize;

    // Parse stat lines to extract file names and summary
    for stat_out in [&stat_output, &stat_cached] {
        if let Ok(ref out) = stat_out {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                // Summary line looks like: "3 files changed, 10 insertions(+), 5 deletions(-)"
                if trimmed.contains("files changed")
                    || trimmed.contains("file changed")
                {
                    if let Some(ins) = extract_stat_number(trimmed, "insertion") {
                        insertions += ins;
                    }
                    if let Some(del) = extract_stat_number(trimmed, "deletion") {
                        deletions += del;
                    }
                } else if trimmed.contains('|') {
                    // File stat line: " src/main.rs | 10 ++---"
                    if let Some(file) = trimmed.split('|').next() {
                        let file = file.trim().to_string();
                        if !file.is_empty() && !changed_files.contains(&file) {
                            changed_files.push(file);
                        }
                    }
                }
            }
        }
    }

    let files_changed = changed_files.len();

    let lines: Vec<String> = all_diff.lines().map(|l| l.to_string()).collect();

    DiffData {
        lines,
        files_changed,
        insertions,
        deletions,
        changed_files,
    }
}

fn extract_stat_number(line: &str, keyword: &str) -> Option<usize> {
    // Find "N insertion(s)" or "N deletion(s)" in a stat summary line
    let parts: Vec<&str> = line.split(',').collect();
    for part in parts {
        let part = part.trim();
        if part.contains(keyword) {
            // Extract leading number
            let num_str: String = part.chars().take_while(|c| c.is_ascii_digit() || c.is_whitespace()).collect();
            if let Ok(n) = num_str.trim().parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
}
