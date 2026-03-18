use crate::notify::{NotificationsConfig, WebhookConfig, WebhookEventType, WebhookPayload};
use crate::templates;
use crate::theme::{Theme, ThemeConfig};
use crate::types::*;
use ratatui::widgets::ListState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// All valid issue types for cycling the type filter
const ALL_TYPES: &[&str] = &["bug", "feature", "task", "chore", "epic"];

// ── CPR stripping regex (byte-level) ──
// Matches cursor-position-report sequences like ESC[1;1R that may leak into PTY output.
static RE_CPR: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    regex::bytes::Regex::new(r"\x1b\[\d+;\d+R").expect("valid CPR regex")
});



const CONFIG_FILE: &str = "obelisk.toml";

#[derive(Serialize, Deserialize, Default)]
struct OrchestratorConfig {
    runtime: Option<String>,
    max_concurrent: Option<usize>,
    auto_spawn: Option<bool>,
    poll_interval_secs: Option<u64>,
    velocity_window: Option<usize>,
    daemon_pty_rows: Option<u16>,
    daemon_pty_cols: Option<u16>,
    output_buffer_lines: Option<usize>,
}

#[derive(Serialize, Deserialize, Default)]
struct ModelsConfig {
    claude: Option<String>,
    codex: Option<String>,
    copilot: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ObeliskConfig {
    orchestrator: Option<OrchestratorConfig>,
    models: Option<ModelsConfig>,
    theme: Option<ThemeConfig>,
    notifications: Option<NotificationsConfig>,
}

pub struct SpawnRequest {
    pub task: BeadTask,
    pub runtime: Runtime,
    pub model: String,
    pub agent_id: usize,
    pub system_prompt: String,
    pub user_prompt: String,
    pub pty_rows: u16,
    pub pty_cols: u16,
}

/// Check whether a binary is reachable on PATH.
fn is_on_path(binary: &str) -> bool {
    #[cfg(windows)]
    let result = std::process::Command::new("where.exe")
        .arg(binary)
        .output();
    #[cfg(not(windows))]
    let result = std::process::Command::new("which")
        .arg(binary)
        .output();
    result.map(|o| o.status.success()).unwrap_or(false)
}

const KNOWN_TOP_KEYS: &[&str] = &["orchestrator", "models", "theme", "notifications"];
const KNOWN_NOTIFICATIONS_KEYS: &[&str] = &["webhook"];
const KNOWN_WEBHOOK_KEYS: &[&str] = &["url", "headers", "events"];
const KNOWN_ORCH_KEYS: &[&str] = &[
    "runtime", "max_concurrent", "auto_spawn", "poll_interval_secs", "velocity_window",
    "daemon_pty_rows", "daemon_pty_cols",
    "output_buffer_lines",
];
const KNOWN_MODELS_KEYS: &[&str] = &["claude", "codex", "copilot"];
const KNOWN_THEME_KEYS: &[&str] = &[
    "preset", "primary", "accent", "secondary", "danger", "info", "warn",
    "dark_bg", "panel_bg", "muted", "bright", "dim_accent",
];
const KNOWN_THEME_PRESETS: &[&str] = &[
    "solarized", "frost", "nord", "ember", "catppuccin", "ash", "gruvbox", "deep",
    "dracula", "dusk", "monokai", "amber", "tokyo-night", "twilight",
    "one-dark", "carbon", "rose-pine", "bloom", "everforest", "moss",
];
const KNOWN_RUNTIMES: &[&str] = &["claude", "codex", "copilot"];

/// Environment variable names for model overrides.
const ENV_MODEL_CLAUDE: &str = "OBELISK_MODEL_CLAUDE";
const ENV_MODEL_CODEX: &str = "OBELISK_MODEL_CODEX";
const ENV_MODEL_COPILOT: &str = "OBELISK_MODEL_COPILOT";

/// Apply environment variable overrides for model selections.
/// Env vars take precedence over obelisk.toml values.
/// Returns a list of change descriptions (empty if no env vars were set).
fn apply_env_model_overrides(model_indices: &mut HashMap<Runtime, usize>) -> Vec<String> {
    let mut changes = Vec::new();
    let env_pairs: &[(Runtime, &str)] = &[
        (Runtime::ClaudeCode, ENV_MODEL_CLAUDE),
        (Runtime::Codex, ENV_MODEL_CODEX),
        (Runtime::Copilot, ENV_MODEL_COPILOT),
    ];
    for &(runtime, env_var) in env_pairs {
        if let Ok(model_str) = std::env::var(env_var) {
            if model_str.is_empty() {
                continue;
            }
            let models = runtime.models();
            match models.iter().position(|m| *m == model_str.as_str()) {
                Some(idx) => {
                    model_indices.insert(runtime, idx);
                    changes.push(format!(
                        "{} model → {} (from {})",
                        runtime.name(),
                        model_str,
                        env_var,
                    ));
                }
                None => {
                    warn!(
                        env_var = env_var,
                        value = %model_str,
                        valid = ?models,
                        event = "env_model_override_invalid",
                        "ignoring invalid model from environment variable"
                    );
                }
            }
        }
    }
    changes
}

/// Validate parsed config and return a list of warnings.
fn validate_config(toml_raw: &str, config: &ObeliskConfig) -> Vec<String> {
    let mut warnings = Vec::new();

    // --- Unknown keys ---
    if let Ok(table) = toml_raw.parse::<toml::Value>() {
        if let Some(top) = table.as_table() {
            for key in top.keys() {
                if !KNOWN_TOP_KEYS.contains(&key.as_str()) {
                    warnings.push(format!("Unknown config section: [{}]", key));
                }
            }
        }
        fn check_sub_keys(
            table: &toml::Value,
            section: &str,
            known: &[&str],
            warnings: &mut Vec<String>,
        ) {
            if let Some(sub) = table.get(section).and_then(|v| v.as_table()) {
                for key in sub.keys() {
                    if !known.contains(&key.as_str()) {
                        warnings.push(format!("Unknown config key: {}.{}", section, key));
                    }
                }
            }
        }
        check_sub_keys(&table, "orchestrator", KNOWN_ORCH_KEYS, &mut warnings);
        check_sub_keys(&table, "models", KNOWN_MODELS_KEYS, &mut warnings);
        check_sub_keys(&table, "theme", KNOWN_THEME_KEYS, &mut warnings);
        check_sub_keys(&table, "notifications", KNOWN_NOTIFICATIONS_KEYS, &mut warnings);
        // Check [notifications.webhook] sub-keys
        if let Some(notif) = table.get("notifications").and_then(|v| v.as_table()) {
            if let Some(wh) = notif.get("webhook").and_then(|v| v.as_table()) {
                for key in wh.keys() {
                    if !KNOWN_WEBHOOK_KEYS.contains(&key.as_str()) {
                        warnings.push(format!(
                            "Unknown config key: notifications.webhook.{}", key
                        ));
                    }
                }
            }
        }
    }

    // --- Orchestrator value ranges ---
    if let Some(orch) = &config.orchestrator {
        if let Some(r) = &orch.runtime {
            if !KNOWN_RUNTIMES.contains(&r.as_str()) {
                warnings.push(format!(
                    "Unknown runtime '{}', defaulting to claude (valid: {})",
                    r,
                    KNOWN_RUNTIMES.join(", ")
                ));
            }
        }
        if let Some(mc) = orch.max_concurrent {
            if mc == 0 {
                warnings.push("max_concurrent=0 is invalid, clamping to 1".into());
            } else if mc > 20 {
                warnings.push(format!(
                    "max_concurrent={} exceeds maximum, clamping to 20", mc
                ));
            }
        }
        if let Some(pi) = orch.poll_interval_secs {
            if pi < 1 {
                warnings.push("poll_interval_secs must be >= 1, using 1".into());
            }
        }
        if let Some(vw) = orch.velocity_window {
            if vw < 2 {
                warnings.push(format!(
                    "velocity_window={} too small, minimum is 2", vw
                ));
            }
        }
        if let Some(rows) = orch.daemon_pty_rows {
            if rows < 10 {
                warnings.push(format!(
                    "daemon_pty_rows={} too small, clamping to 10", rows
                ));
            } else if rows > 500 {
                warnings.push(format!(
                    "daemon_pty_rows={} too large, clamping to 500", rows
                ));
            }
        }
        if let Some(cols) = orch.daemon_pty_cols {
            if cols < 40 {
                warnings.push(format!(
                    "daemon_pty_cols={} too small, clamping to 40", cols
                ));
            } else if cols > 500 {
                warnings.push(format!(
                    "daemon_pty_cols={} too large, clamping to 500", cols
                ));
            }
        }
        if let Some(obl) = orch.output_buffer_lines {
            if obl < 100 {
                warnings.push(format!(
                    "output_buffer_lines={} too small, minimum is 100", obl
                ));
            } else if obl > 1_000_000 {
                warnings.push(format!(
                    "output_buffer_lines={} exceeds maximum, clamping to 1000000", obl
                ));
            }
        }
    }

    // --- Unknown model names ---
    if let Some(models) = &config.models {
        let pairs: &[(&str, &Option<String>, Runtime)] = &[
            ("claude", &models.claude, Runtime::ClaudeCode),
            ("codex", &models.codex, Runtime::Codex),
            ("copilot", &models.copilot, Runtime::Copilot),
        ];
        for (name, model_opt, runtime) in pairs {
            if let Some(model_str) = model_opt {
                if !runtime.models().contains(&model_str.as_str()) {
                    warnings.push(format!(
                        "Unknown model '{}' for {}, valid: [{}]",
                        model_str,
                        name,
                        runtime.models().join(", ")
                    ));
                }
            }
        }
    }

    // --- Theme preset ---
    if let Some(theme) = &config.theme {
        if let Some(preset) = &theme.preset {
            if !KNOWN_THEME_PRESETS.contains(&preset.as_str()) {
                warnings.push(format!(
                    "Unknown theme preset '{}', valid: [{}]",
                    preset,
                    KNOWN_THEME_PRESETS.join(", ")
                ));
            }
        }
    }

    // --- Webhook config ---
    if let Some(notif) = &config.notifications {
        if let Some(wh) = &notif.webhook {
            warnings.extend(wh.validate());
        }
    }

    // --- Runtime binary on PATH ---
    let runtime_str = config
        .orchestrator
        .as_ref()
        .and_then(|o| o.runtime.as_deref())
        .unwrap_or("claude");
    let binary = match runtime_str {
        "codex" => "codex",
        "copilot" => "copilot",
        _ => "claude",
    };
    if !is_on_path(binary) {
        warnings.push(format!(
            "Runtime '{}' ({}) not found on PATH", runtime_str, binary
        ));
    }

    // --- bd (beads CLI) on PATH ---
    if !is_on_path("bd") {
        warnings.push("'bd' not found on PATH — issue polling will fail".into());
    }

    warnings
}

/// Scan a chunk of PTY text for phase-indicating patterns.
/// Returns the most advanced phase detected, or None if no marker found.
/// Patterns are checked in reverse phase order so the highest phase wins.
fn detect_phase(text: &str) -> Option<AgentPhase> {
    if text.contains("bd close") {
        return Some(AgentPhase::Closing);
    }
    if text.contains("--no-ff") {
        return Some(AgentPhase::Merging);
    }
    if text.contains("cargo test")
        || text.contains("cargo check")
        || text.contains("cargo clippy")
    {
        return Some(AgentPhase::Verifying);
    }
    if text.contains("--notes") {
        return Some(AgentPhase::Implementing);
    }
    if text.contains("git worktree add") {
        return Some(AgentPhase::Worktree);
    }
    if text.contains("--claim") {
        return Some(AgentPhase::Claiming);
    }
    None
}

/// Detect common error patterns in PTY output lines.
/// Returns a list of human-readable error summaries.
fn detect_error_patterns(lines: &[&str]) -> Vec<String> {
    let mut errors = Vec::new();

    let mut compile_errors = 0u32;
    let mut test_failures = 0u32;
    let mut panics = 0u32;
    let mut permission_denied = false;
    let mut merge_conflicts = false;

    for line in lines {
        let lower = line.to_lowercase();
        // Rust compilation errors
        if lower.contains("error[e") || (lower.starts_with("error") && lower.contains("-->")) {
            compile_errors += 1;
        }
        // Test failures
        if lower.contains("test result: failed")
            || lower.contains("failures:")
            || lower.contains("failed")
                && (lower.contains("test") || lower.contains("assert"))
        {
            test_failures += 1;
        }
        // Panics / stack traces
        if lower.contains("thread '") && lower.contains("panicked") {
            panics += 1;
        }
        if lower.contains("stack backtrace") {
            panics = panics.max(1);
        }
        // Permission / access errors
        if lower.contains("permission denied") || lower.contains("access denied") {
            permission_denied = true;
        }
        // Git merge conflicts
        if lower.contains("merge conflict") || lower.contains("unmerged paths") {
            merge_conflicts = true;
        }
    }

    if compile_errors > 0 {
        errors.push(format!("- Compilation errors detected ({} error lines)", compile_errors));
    }
    if test_failures > 0 {
        errors.push(format!("- Test failures detected ({} failure indicators)", test_failures));
    }
    if panics > 0 {
        errors.push(format!("- Panic / crash detected ({} panic(s))", panics));
    }
    if permission_denied {
        errors.push("- Permission denied errors".to_string());
    }
    if merge_conflicts {
        errors.push("- Git merge conflicts".to_string());
    }

    errors
}

/// Extract failure context from a failed agent's PTY output.
/// Returns a formatted string with exit code, last N lines of output,
/// and any detected error patterns.
fn extract_failure_context(agent: &AgentInstance, context_lines: usize) -> String {
    let mut sections = Vec::new();

    // Exit code
    if let Some(code) = agent.exit_code {
        sections.push(format!("Exit code: {}", code));
    }

    // Last N lines of PTY output
    let total = agent.output.len();
    let take = total.min(context_lines);
    let tail_lines: Vec<&str> = agent
        .output
        .iter()
        .skip(total.saturating_sub(take))
        .map(|s| s.as_str())
        .collect();

    // Detect error patterns in the tail
    let patterns = detect_error_patterns(&tail_lines);
    if !patterns.is_empty() {
        sections.push(format!("Detected errors:\n{}", patterns.join("\n")));
    }

    // Include the raw tail output
    if !tail_lines.is_empty() {
        sections.push(format!(
            "Last {} lines of output:\n```\n{}\n```",
            tail_lines.len(),
            tail_lines.join("\n")
        ));
    }

    sections.join("\n\n")
}

pub struct App {
    pub ready_tasks: Vec<BeadTask>,
    pub agents: Vec<AgentInstance>,
    pub event_log: VecDeque<LogEntry>,
    pub log_category_filter: Option<LogCategory>,

    pub active_view: View,
    pub focus: Focus,
    pub task_list_state: ListState,
    pub agent_list_state: ListState,
    pub log_scroll: usize,

    pub selected_runtime: Runtime,
    pub auto_spawn: bool,
    pub max_concurrent: usize,
    pub poll_interval_secs: u64,
    pub daemon_pty_rows: u16,
    pub daemon_pty_cols: u16,
    pub poll_countdown: f64,

    pub should_quit: bool,
    pub next_unit: usize,
    pub claimed_task_ids: HashSet<String>,

    pub total_completed: u32,
    pub total_failed: u32,
    pub max_retries: u32,
    /// Number of PTY output lines to include in failure context on retry
    pub retry_context_lines: usize,

    pub selected_agent_id: Option<usize>,
    /// None = auto-follow (pinned to bottom), Some(n) = manual scroll at line n from top
    pub agent_output_scroll: Option<usize>,

    /// Directory for per-type prompt templates
    pub template_dir: PathBuf,

    pub frame_count: u64,

    // Throughput tracking (lines per second over last 60 ticks)
    pub throughput_history: VecDeque<u16>,
    pub lines_this_tick: u16,

    // Alert system
    pub alert_message: Option<(String, u64)>, // (message, frame_to_expire)

    // Per-runtime model selection
    pub model_indices: HashMap<Runtime, usize>,

    // PTY state per agent: terminal parser + writer + master
    pub pty_states: HashMap<usize, PtyHandle>,

    // Interactive terminal mode — keystrokes go to the agent's PTY
    pub interactive_mode: bool,


    // Last known PTY inner area (rows, cols) — used to avoid redundant resizes
    pub last_pty_size: (u16, u16),

    // Help overlay toggle
    pub show_help: bool,

    // Poll health tracking
    pub last_poll_ok: bool,
    pub last_poll_error: Option<String>,
    pub consecutive_poll_failures: u32,

    // Ready queue sort/filter state
    pub sort_mode: SortMode,
    /// Active type filters — empty means "show all"
    pub type_filter: HashSet<String>,
    /// Active priority range filter — None means "show all"
    pub priority_filter: Option<std::ops::RangeInclusive<i32>>,
    /// Index into ALL_TYPES for the "cycle type filter" keybinding
    pub type_filter_cursor: usize,

    // Session tracking
    pub session_id: String,
    pub session_started_at: chrono::DateTime<chrono::Local>,

    // Loaded history sessions (from .beads/obelisk_sessions.jsonl)
    pub history_sessions: Vec<SessionRecord>,
    pub history_scroll: usize,

    // Agent list status filter
    pub agent_status_filter: AgentStatusFilter,

    // Search state (in AgentDetail observe mode)
    pub search_active: bool,
    pub search_query: String,
    pub search_matches: Vec<(usize, usize)>, // (screen_row, screen_col)
    pub search_current_idx: usize,

    // Diff panel state
    pub show_diff_panel: bool,
    pub diff_data: Option<DiffData>,
    pub diff_scroll: usize,
    /// Frame count at which the last diff poll was triggered
    pub diff_last_poll_frame: u64,

    // Desktop notifications
    pub notifications_enabled: bool,

    // Webhook notifications config
    pub webhook_config: WebhookConfig,
    pub webhook_failure_tx: mpsc::UnboundedSender<String>,
    pub webhook_failure_rx: mpsc::UnboundedReceiver<String>,

    // Split-pane view state
    /// Agent IDs pinned to each pane slot (up to 4)
    pub split_pane_agents: [Option<usize>; 4],
    /// Which pane (0-3) is currently focused
    pub split_pane_focus: usize,
    /// Output scroll per pane
    pub split_pane_scroll: [Option<usize>; 4],
    /// Rotation offset for cycling through agents when > 4 are running
    pub split_pane_rotation_offset: usize,

    // Jump-to-issue mode
    pub jump_active: bool,
    pub jump_query: String,

    // Velocity chart — configurable window (number of data points)
    pub velocity_window_size: usize,

    // Maximum number of output lines retained per agent
    pub output_buffer_lines: usize,

    // Kill confirmation dialog — Some(agent_id) means dialog is visible
    pub confirm_kill_agent_id: Option<usize>,

    // Mark-complete confirmation dialog — Some(agent_id) means dialog is visible
    pub confirm_complete_agent_id: Option<usize>,

    // Quit confirmation dialog — true when agents are still running and user pressed q
    pub confirm_quit: bool,

    // Tab bar badge: event log count at last visit (for "unread" badge)
    pub event_log_seen_count: usize,

    // Worktree overview panel state
    pub worktree_entries: Vec<WorktreeEntry>,
    pub worktree_list_state: ListState,
    pub worktree_sort_mode: WorktreeSortMode,
    /// Frame count at which the last worktree scan was triggered
    pub worktree_last_scan_frame: u64,

    // Recent completions feed (Dashboard panel)
    pub recent_completions: VecDeque<CompletionRecord>,

    // Color theme
    pub theme: Theme,
    pub theme_config: ThemeConfig,

    // Blocked/incoming issues panel
    pub blocked_tasks: Vec<BlockedTask>,
    pub blocked_list_state: ListState,

    // Layout areas for mouse hit-testing
    pub layout_areas: LayoutAreas,

    // Dependency graph view state
    pub dep_graph_nodes: Vec<DepNode>,
    pub dep_graph_rows: Vec<DepGraphRow>,
    pub dep_graph_list_state: ListState,
    pub dep_graph_collapsed: HashSet<String>,
    pub dep_graph_last_poll_frame: u64,

    // Config hot-reload state
    /// Last known modification time of obelisk.toml
    pub config_mtime: Option<std::time::SystemTime>,
    /// Frame count at which the last config mtime check was performed
    pub config_check_frame: u64,

    // Repository name for display in title bar
    pub repo_name: String,

    // Issue creation form overlay
    pub issue_creation_active: bool,
    pub issue_creation_form: IssueCreationForm,

    // Merge queue — tracks agents in the Merging phase to serialize merges
    pub merge_queue: VecDeque<MergeQueueEntry>,
}

fn compute_search_matches(screen: &vt100::Screen, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let (rows, cols) = screen.size();
    let mut matches = Vec::new();

    for row in 0..rows as usize {
        let row_text: String = (0..cols)
            .map(|col| {
                screen
                    .cell(row as u16, col)
                    .and_then(|c| c.contents().chars().next())
                    .unwrap_or(' ')
            })
            .collect();

        let row_lower = row_text.to_lowercase();
        let mut byte_start = 0usize;
        while byte_start < row_lower.len() {
            if let Some(byte_idx) = row_lower[byte_start..].find(&query_lower) {
                let abs_byte = byte_start + byte_idx;
                let char_col = row_lower[..abs_byte].chars().count();
                matches.push((row, char_col));
                byte_start = abs_byte + 1;
            } else {
                break;
            }
        }
    }
    matches
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect the repository name from the git remote URL or fall back to the
/// working directory name.
fn detect_repo_name() -> String {
    // Try git remote URL first
    if let Ok(output) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Handle SSH: git@github.com:user/repo.git
            // Handle HTTPS: https://github.com/user/repo.git
            if let Some(name) = url
                .rsplit('/')
                .next()
                .or_else(|| url.rsplit(':').next())
            {
                let name = name.trim_end_matches(".git");
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    // Fallback: current directory name
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

mod init;
mod session;
mod queue;
mod agents;
mod navigation;
mod worktree;
mod display;
mod pty;
#[cfg(test)]
mod tests;

pub use session::load_history_sessions;
use session::generate_session_id;
