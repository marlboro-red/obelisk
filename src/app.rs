use crate::notify::{NotificationsConfig, WebhookConfig, WebhookEventType, WebhookPayload};
use crate::templates;
use crate::theme::{Theme, ThemeConfig};
use crate::types::*;
use ratatui::widgets::ListState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};

// All valid issue types for cycling the type filter
const ALL_TYPES: &[&str] = &["bug", "feature", "task", "chore", "epic"];

// ── CPR stripping regex (byte-level) ──
// Matches cursor-position-report sequences like ESC[1;1R that may leak into PTY output.
static RE_CPR: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    regex::bytes::Regex::new(r"\x1b\[\d+;\d+R").unwrap()
});



const CONFIG_FILE: &str = "obelisk.toml";

#[derive(Serialize, Deserialize, Default)]
struct OrchestratorConfig {
    runtime: Option<String>,
    max_concurrent: Option<usize>,
    auto_spawn: Option<bool>,
    poll_interval_secs: Option<u64>,
    velocity_window: Option<usize>,
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

impl App {
    pub fn new() -> Self {
        let config_exists = std::path::Path::new(CONFIG_FILE).exists();
        let (config, config_warnings) = if config_exists {
            match std::fs::read_to_string(CONFIG_FILE) {
                Ok(raw) => match toml::from_str::<ObeliskConfig>(&raw) {
                    Ok(cfg) => {
                        let warnings = validate_config(&raw, &cfg);
                        (cfg, warnings)
                    }
                    Err(e) => (
                        ObeliskConfig::default(),
                        vec![format!("Failed to parse {}: {}", CONFIG_FILE, e)],
                    ),
                },
                Err(e) => (
                    ObeliskConfig::default(),
                    vec![format!("Failed to read {}: {}", CONFIG_FILE, e)],
                ),
            }
        } else {
            (ObeliskConfig::default(), vec![])
        };

        let mut selected_runtime = Runtime::ClaudeCode;
        let mut max_concurrent = 10usize;
        let mut auto_spawn = false;
        let mut poll_interval_secs = 30u64;
        let mut velocity_window_size = 24usize;
        let mut model_indices: HashMap<Runtime, usize> = HashMap::from([
            (Runtime::ClaudeCode, 0),
            (Runtime::Codex, 0),
            (Runtime::Copilot, 0),
        ]);

        if let Some(orch) = &config.orchestrator {
            if let Some(r) = &orch.runtime {
                selected_runtime = match r.as_str() {
                    "claude" => Runtime::ClaudeCode,
                    "codex" => Runtime::Codex,
                    "copilot" => Runtime::Copilot,
                    _ => Runtime::ClaudeCode,
                };
            }
            if let Some(mc) = orch.max_concurrent {
                max_concurrent = mc.clamp(1, 20);
            }
            if let Some(asp) = orch.auto_spawn {
                auto_spawn = asp;
            }
            if let Some(pi) = orch.poll_interval_secs {
                poll_interval_secs = pi.max(1);
            }
            if let Some(vw) = orch.velocity_window {
                velocity_window_size = vw.max(2); // minimum 2 data points
            }
        }

        if let Some(models) = &config.models {
            let pairs: &[(Runtime, &Option<String>)] = &[
                (Runtime::ClaudeCode, &models.claude),
                (Runtime::Codex, &models.codex),
                (Runtime::Copilot, &models.copilot),
            ];
            for (runtime, model_opt) in pairs {
                if let Some(model_str) = model_opt {
                    let idx = runtime
                        .models()
                        .iter()
                        .position(|m| *m == model_str.as_str())
                        .unwrap_or(0);
                    model_indices.insert(*runtime, idx);
                }
            }
        }

        let session_id = generate_session_id();
        let history_sessions = load_history_sessions();
        let last_session_summary = history_sessions.last().map(|last| format!(
            "Last session {}: {} completed, {} failed ({} agents)",
            &last.session_id[..8.min(last.session_id.len())],
            last.total_completed,
            last.total_failed,
            last.agents.len(),
        ));

        let theme_config = config.theme.clone().unwrap_or_default();
        let theme = Theme::from_config(&theme_config);
        let webhook_config = config
            .notifications
            .as_ref()
            .and_then(|n| n.webhook.clone())
            .unwrap_or_default();

        let poll_countdown = poll_interval_secs as f64;
        let mut app = Self {
            ready_tasks: Vec::new(),
            agents: Vec::new(),
            event_log: VecDeque::with_capacity(500),
            log_category_filter: None,
            active_view: View::Dashboard,
            focus: Focus::ReadyQueue,
            task_list_state: ListState::default(),
            agent_list_state: ListState::default(),
            log_scroll: 0,
            selected_runtime,
            auto_spawn,
            max_concurrent,
            poll_interval_secs,
            poll_countdown,
            should_quit: false,
            next_unit: 0,
            claimed_task_ids: HashSet::new(),
            total_completed: 0,
            total_failed: 0,
            max_retries: 3,
            retry_context_lines: 80,
            selected_agent_id: None,
            agent_output_scroll: None,
            template_dir: templates::default_template_dir(),
            frame_count: 0,
            throughput_history: VecDeque::from(vec![0; 60]),
            lines_this_tick: 0,
            alert_message: None,
            model_indices,
            pty_states: HashMap::new(),
            interactive_mode: false,
            last_pty_size: (24, 120),
            show_help: false,
            last_poll_ok: true,
            last_poll_error: None,
            consecutive_poll_failures: 0,
            sort_mode: SortMode::Priority,
            type_filter: HashSet::new(),
            priority_filter: None,
            type_filter_cursor: 0,
            session_id,
            session_started_at: chrono::Local::now(),
            history_sessions,
            history_scroll: 0,
            agent_status_filter: AgentStatusFilter::All,
            search_active: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current_idx: 0,
            show_diff_panel: false,
            diff_data: None,
            diff_scroll: 0,
            diff_last_poll_frame: 0,
            notifications_enabled: true,
            webhook_config,
            split_pane_agents: [None; 4],
            split_pane_focus: 0,
            split_pane_scroll: [None; 4],
            split_pane_rotation_offset: 0,
            jump_active: false,
            jump_query: String::new(),
            velocity_window_size,
            confirm_kill_agent_id: None,
            confirm_complete_agent_id: None,
            confirm_quit: false,
            event_log_seen_count: 0,
            worktree_entries: Vec::new(),
            worktree_list_state: ListState::default(),
            worktree_sort_mode: WorktreeSortMode::Age,
            worktree_last_scan_frame: 0,
            recent_completions: VecDeque::with_capacity(10),
            theme,
            theme_config,
            blocked_tasks: Vec::new(),
            blocked_list_state: ListState::default(),
            layout_areas: LayoutAreas::default(),
            dep_graph_nodes: Vec::new(),
            dep_graph_rows: Vec::new(),
            dep_graph_list_state: ListState::default(),
            dep_graph_collapsed: HashSet::new(),
            dep_graph_last_poll_frame: 0,
            config_mtime: std::fs::metadata(CONFIG_FILE)
                .and_then(|m| m.modified())
                .ok(),
            config_check_frame: 0,
            repo_name: detect_repo_name(),
            issue_creation_active: false,
            issue_creation_form: IssueCreationForm::new(),
            merge_queue: VecDeque::new(),
        };

        // Seed recent completions from history sessions (most recent agents last)
        for session in app.history_sessions.iter().rev().take(3) {
            for agent in session.agents.iter().rev() {
                if app.recent_completions.len() >= 10 {
                    break;
                }
                app.recent_completions.push_front(CompletionRecord {
                    task_id: agent.task_id.clone(),
                    title: agent.task_id.clone(), // history doesn't store title
                    runtime: agent.runtime.clone(),
                    model: agent.model.clone(),
                    elapsed_secs: agent.elapsed_secs,
                    success: agent.status == "Completed",
                                    });
            }
        }
        app.log(LogCategory::System, "Orchestrator initialized".into());
        if config_exists {
            app.log(LogCategory::System, format!("Config loaded from {}", CONFIG_FILE));
        } else {
            app.log(LogCategory::System, "No config file found, using defaults".into());
        }
        for warning in &config_warnings {
            app.log(LogCategory::Alert, format!("Config: {}", warning));
        }
        if !config_warnings.is_empty() {
            app.alert_message = Some((
                format!(
                    "{} CONFIG WARNING(S) — check event log (Tab 3)",
                    config_warnings.len()
                ),
                100, // ~10 seconds at 10fps
            ));
        }
        app.log(LogCategory::System, "System online".into());
        if let Some(summary) = last_session_summary {
            app.log(LogCategory::System, summary);
        }
        app
    }

    pub fn save_config(&mut self) {
        let runtime_str = match self.selected_runtime {
            Runtime::ClaudeCode => "claude",
            Runtime::Codex => "codex",
            Runtime::Copilot => "copilot",
        };
        let config = ObeliskConfig {
            orchestrator: Some(OrchestratorConfig {
                runtime: Some(runtime_str.to_string()),
                max_concurrent: Some(self.max_concurrent),
                auto_spawn: Some(self.auto_spawn),
                poll_interval_secs: Some(self.poll_interval_secs),
                velocity_window: Some(self.velocity_window_size),
            }),
            models: Some(ModelsConfig {
                claude: Some(self.selected_model_for(Runtime::ClaudeCode).to_string()),
                codex: Some(self.selected_model_for(Runtime::Codex).to_string()),
                copilot: Some(self.selected_model_for(Runtime::Copilot).to_string()),
            }),
            theme: Some(self.theme_config.clone()),
            notifications: if self.webhook_config != WebhookConfig::default() {
                Some(NotificationsConfig {
                    webhook: Some(self.webhook_config.clone()),
                })
            } else {
                None
            },
        };
        if let Ok(toml_str) = toml::to_string_pretty(&config) {
            if std::fs::write(CONFIG_FILE, toml_str).is_ok() {
                self.log(LogCategory::System, format!("Config saved to {}", CONFIG_FILE));
                // Update stored mtime so the watcher doesn't treat our own save as an
                // external change.
                self.config_mtime = std::fs::metadata(CONFIG_FILE)
                    .and_then(|m| m.modified())
                    .ok();
            }
        }
    }

    /// Check if `obelisk.toml` has been modified since we last loaded it, and
    /// if so, re-read and apply the new values.  Called periodically from the
    /// tick handler (~every 2 seconds).  Returns `true` if config was reloaded
    /// (callers may need to propagate poll_interval changes).
    pub fn check_config_reload(&mut self) -> bool {
        // Only check every ~2s (20 ticks at 100ms)
        if self.frame_count.saturating_sub(self.config_check_frame) < 20 {
            return false;
        }
        self.config_check_frame = self.frame_count;

        let current_mtime = match std::fs::metadata(CONFIG_FILE).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return false, // file doesn't exist or unreadable
        };

        if self.config_mtime == Some(current_mtime) {
            return false; // unchanged
        }

        // File was modified — reload
        let raw = match std::fs::read_to_string(CONFIG_FILE) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, event = "config_reload_failed", "config reload failed: could not read file");
                self.log(
                    LogCategory::System,
                    format!("[WARN] Config reload failed (read): {}", e),
                );
                return false;
            }
        };
        let cfg: ObeliskConfig = match toml::from_str(&raw) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, event = "config_parse_failed", "config reload failed: TOML parse error");
                self.log(
                    LogCategory::System,
                    format!("[WARN] Config reload failed (parse): {}", e),
                );
                return false;
            }
        };

        // Validate — log warnings but still apply
        let warnings = validate_config(&raw, &cfg);
        for w in &warnings {
            self.log(LogCategory::System, format!("[WARN] {}", w));
        }

        let mut changes: Vec<String> = Vec::new();

        // ── Orchestrator settings ──
        if let Some(orch) = &cfg.orchestrator {
            if let Some(r) = &orch.runtime {
                let new_rt = match r.as_str() {
                    "claude" => Runtime::ClaudeCode,
                    "codex" => Runtime::Codex,
                    "copilot" => Runtime::Copilot,
                    _ => self.selected_runtime,
                };
                if new_rt != self.selected_runtime {
                    self.selected_runtime = new_rt;
                    changes.push(format!("runtime → {}", new_rt.name()));
                }
            }
            if let Some(mc) = orch.max_concurrent {
                let mc = mc.clamp(1, 20);
                if mc != self.max_concurrent {
                    self.max_concurrent = mc;
                    changes.push(format!("max_concurrent → {}", mc));
                }
            }
            if let Some(asp) = orch.auto_spawn {
                if asp != self.auto_spawn {
                    self.auto_spawn = asp;
                    changes.push(format!(
                        "auto_spawn → {}",
                        if asp { "on" } else { "off" }
                    ));
                }
            }
            if let Some(pi) = orch.poll_interval_secs {
                let pi = pi.max(1);
                if pi != self.poll_interval_secs {
                    self.poll_interval_secs = pi;
                    changes.push(format!("poll_interval → {}s", pi));
                }
            }
            if let Some(vw) = orch.velocity_window {
                let vw = vw.max(2);
                if vw != self.velocity_window_size {
                    self.velocity_window_size = vw;
                    changes.push(format!("velocity_window → {}", vw));
                }
            }
        }

        // ── Model settings ──
        if let Some(models) = &cfg.models {
            let pairs: &[(Runtime, &Option<String>)] = &[
                (Runtime::ClaudeCode, &models.claude),
                (Runtime::Codex, &models.codex),
                (Runtime::Copilot, &models.copilot),
            ];
            for (runtime, model_opt) in pairs {
                if let Some(model_str) = model_opt {
                    let idx = runtime
                        .models()
                        .iter()
                        .position(|m| *m == model_str.as_str())
                        .unwrap_or(0);
                    let prev = self.model_indices.get(runtime).copied().unwrap_or(0);
                    if idx != prev {
                        self.model_indices.insert(*runtime, idx);
                        changes.push(format!(
                            "{} model → {}",
                            runtime.name(),
                            model_str
                        ));
                    }
                }
            }
        }

        // ── Theme settings ──
        let new_theme_config = cfg.theme.unwrap_or_default();
        if new_theme_config != self.theme_config {
            self.theme = Theme::from_config(&new_theme_config);
            self.theme_config = new_theme_config;
            changes.push("theme updated".into());
        }

        // ── Webhook settings ──
        let new_webhook = cfg
            .notifications
            .as_ref()
            .and_then(|n| n.webhook.clone())
            .unwrap_or_default();
        if new_webhook != self.webhook_config {
            self.webhook_config = new_webhook;
            changes.push("webhook config updated".into());
        }

        self.config_mtime = Some(current_mtime);

        if changes.is_empty() {
            debug!(event = "config_reload_noop", "config file changed but no effective differences");
            self.log(
                LogCategory::System,
                "Config file changed but no effective differences".into(),
            );
        } else {
            info!(
                changes = %changes.join(", "),
                change_count = changes.len(),
                event = "config_reloaded",
                "configuration hot-reloaded"
            );
            self.log(
                LogCategory::System,
                format!("Config reloaded: {}", changes.join(", ")),
            );
        }
        true
    }

    /// Build a SessionRecord from the current session and append it to the
    /// persistent log file `.beads/obelisk_sessions.jsonl`.
    pub fn save_session(&self) {
        let ended_at = chrono::Local::now();
        let agents: Vec<SessionAgent> = self
            .agents
            .iter()
            .map(|a| {
                let (input_tokens, output_tokens, estimated_cost_usd) = a
                    .usage
                    .as_ref()
                    .map(|u| (u.input_tokens + u.cache_creation_tokens + u.cache_read_tokens, u.output_tokens, u.cost_usd))
                    .unwrap_or((0, 0, 0.0));
                SessionAgent {
                    task_id: a.task.id.clone(),
                    runtime: a.runtime.name().to_string(),
                    model: a.model.clone(),
                    elapsed_secs: a.elapsed_secs,
                    status: match a.status {
                        AgentStatus::Starting => "Starting".to_string(),
                        AgentStatus::Running => "Running".to_string(),
                        AgentStatus::Completed => "Completed".to_string(),
                        AgentStatus::Failed => "Failed".to_string(),
                    },
                    input_tokens,
                    output_tokens,
                    estimated_cost_usd,
                }
            })
            .collect();

        let total_cost_usd: f64 = agents.iter().map(|a| a.estimated_cost_usd).sum();

        let record = SessionRecord {
            session_id: self.session_id.clone(),
            started_at: self.session_started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            ended_at: ended_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            total_completed: self.total_completed,
            total_failed: self.total_failed,
            total_cost_usd,
            agents,
        };

        if let Ok(json) = serde_json::to_string(&record) {
            let path = sessions_file_path();
            // Ensure parent directory exists
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = writeln!(f, "{}", json);
                info!(
                    session_id = %self.session_id,
                    total_completed = self.total_completed,
                    total_failed = self.total_failed,
                    agents_count = self.agents.len(),
                    event = "session_saved",
                    "session record persisted"
                );
            }
        }
    }

    pub fn log(&mut self, category: LogCategory, message: String) {
        let entry = LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            category,
            message,
        };
        self.event_log.push_front(entry);
        if self.event_log.len() > 500 {
            self.event_log.pop_back();
        }
    }

    pub fn active_agent_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .count()
    }

    pub fn count_running(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .count()
    }

    pub fn count_completed(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Completed))
            .count()
    }

    pub fn count_failed(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Failed))
            .count()
    }

    pub fn on_tick(&mut self) {
        self.frame_count += 1;
        if self.poll_countdown > 0.0 {
            self.poll_countdown -= 0.1;
            if self.poll_countdown < 0.0 {
                self.poll_countdown = 0.0;
            }
        }
        for agent in &mut self.agents {
            if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
                agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            }
        }

        // Update throughput history every 10 frames (~1 second)
        if self.frame_count.is_multiple_of(10) {
            self.throughput_history.push_back(self.lines_this_tick);
            if self.throughput_history.len() > 60 {
                self.throughput_history.pop_front();
            }
            self.lines_this_tick = 0;
        }

        // Clear expired alerts
        if let Some((_, expires)) = &self.alert_message {
            if self.frame_count > *expires {
                self.alert_message = None;
            }
        }

        // Periodically flush PTY logs to disk for running agents (~every 30s)
        if self.frame_count.is_multiple_of(300) {
            let running_ids: Vec<usize> = self
                .agents
                .iter()
                .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
                .map(|a| a.id)
                .collect();
            for id in running_ids {
                self.persist_agent_pty_log(id);
            }
        }

    }

    pub fn on_poll_failed(&mut self, error: String) {
        self.consecutive_poll_failures += 1;
        self.last_poll_ok = false;
        self.last_poll_error = Some(error.clone());
        self.poll_countdown = self.poll_interval_secs as f64;

        self.log(
            LogCategory::Alert,
            format!("Poll failed ({}): {}", self.consecutive_poll_failures, error),
        );

        if self.consecutive_poll_failures >= 3 {
            self.log(
                LogCategory::Alert,
                "Repeated poll failures — check dolt server status".into(),
            );
        }
    }

    pub fn on_poll_result(&mut self, tasks: Vec<BeadTask>) {
        // Recover from previous failures
        if !self.last_poll_ok {
            self.log(LogCategory::Poll, "Poll recovered — bd CLI is responding again".into());
        }
        self.last_poll_ok = true;
        self.last_poll_error = None;
        self.consecutive_poll_failures = 0;

        self.poll_countdown = self.poll_interval_secs as f64;
        let new_tasks: Vec<BeadTask> = tasks
            .into_iter()
            .filter(|t| !self.claimed_task_ids.contains(&t.id))
            // Epics are containers, not directly workable — only their children
            // (tasks, bugs, features, etc.) should be assigned to agents.
            .filter(|t| !t.is_epic())
            .collect();
        let new_count = new_tasks
            .iter()
            .filter(|t| !self.ready_tasks.iter().any(|rt| rt.id == t.id))
            .count();
        if new_count > 0 {
            let new_ids: Vec<&str> = new_tasks
                .iter()
                .filter(|t| !self.ready_tasks.iter().any(|rt| rt.id == t.id))
                .map(|t| t.id.as_str())
                .collect();
            info!(
                new_count,
                new_task_ids = %new_ids.join(","),
                total_ready = new_tasks.len(),
                event = "new_tasks_detected",
                "new beads tasks available"
            );
            self.log(
                LogCategory::Incoming,
                format!("{} new ready task(s) detected", new_count),
            );
            self.alert_message = Some((
                format!("INCOMING — {} NEW TASK(S) DETECTED", new_count),
                self.frame_count + 50, // ~5 seconds
            ));
            // Notify for new P0/P1 tasks
            if self.notifications_enabled {
                let high_prio_tasks: Vec<&BeadTask> = new_tasks
                    .iter()
                    .filter(|t| {
                        !self.ready_tasks.iter().any(|rt| rt.id == t.id)
                            && t.priority.unwrap_or(3) <= 1
                    })
                    .collect();
                if !high_prio_tasks.is_empty() {
                    let ids: Vec<&str> = high_prio_tasks.iter().map(|t| t.id.as_str()).collect();
                    crate::notify::send_notification(
                        "High-Priority Task Available",
                        &format!("P0/P1 ready: {}", ids.join(", ")),
                    );
                    crate::notify::send_bell();
                    // Send webhook for each high-priority task
                    for task in &high_prio_tasks {
                        crate::notify::send_webhook(
                            &self.webhook_config,
                            WebhookEventType::HighPriorityReady,
                            WebhookPayload {
                                event: "high_priority_ready".into(),
                                issue_id: task.id.clone(),
                                title: task.title.clone(),
                                status: task.status.clone(),
                                runtime: None,
                                elapsed_secs: None,
                                exit_code: None,
                                failure_details: None,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                    }
                }
            }
        }
        self.ready_tasks = new_tasks;
        self.sort_ready_tasks();
        self.log(
            LogCategory::Poll,
            format!(
                "Scan complete: {} ready, {} active",
                self.ready_tasks.len(),
                self.active_agent_count()
            ),
        );
    }

    pub fn on_blocked_poll_result(&mut self, tasks: Vec<BlockedTask>) {
        self.blocked_tasks = tasks;
        // Sort by priority (highest first), then by remaining deps (most blocked first)
        self.blocked_tasks.sort_by(|a, b| {
            let pa = a.task.priority.unwrap_or(3);
            let pb = b.task.priority.unwrap_or(3);
            pa.cmp(&pb)
                .then_with(|| b.remaining_deps.cmp(&a.remaining_deps))
                .then_with(|| a.task.id.cmp(&b.task.id))
        });
    }

    /// Sort `ready_tasks` in-place according to the current `sort_mode`.
    /// Secondary sort is always by creation time (id lexicographically, which
    /// uses a timestamp prefix in the bead ID scheme).
    pub fn sort_ready_tasks(&mut self) {
        match self.sort_mode {
            SortMode::Priority => {
                self.ready_tasks.sort_by(|a, b| {
                    let pa = a.priority.unwrap_or(3);
                    let pb = b.priority.unwrap_or(3);
                    pa.cmp(&pb).then_with(|| a.id.cmp(&b.id))
                });
            }
            SortMode::Type => {
                self.ready_tasks.sort_by(|a, b| {
                    let ta = a.issue_type.as_deref().unwrap_or("task");
                    let tb = b.issue_type.as_deref().unwrap_or("task");
                    ta.cmp(tb)
                        .then_with(|| a.priority.unwrap_or(3).cmp(&b.priority.unwrap_or(3)))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortMode::Age => {
                // Smaller id ≈ older task (id contains a timestamp token)
                self.ready_tasks.sort_by(|a, b| a.id.cmp(&b.id));
            }
            SortMode::Name => {
                self.ready_tasks.sort_by(|a, b| {
                    a.title.to_lowercase().cmp(&b.title.to_lowercase())
                });
            }
        }
    }

    /// Cycle to the next sort mode and re-sort.
    pub fn cycle_sort_mode(&mut self) {
        self.sort_mode = self.sort_mode.next();
        self.sort_ready_tasks();
        self.log(
            LogCategory::System,
            format!("Queue sort: {}", self.sort_mode.label()),
        );
    }

    /// Toggle the type filter for the next type in the cycle.
    /// Pressing 'F' toggles each type one at a time. When all types have been
    /// toggled on, the next press clears the filter (show all).
    pub fn cycle_type_filter(&mut self) {
        let all_count = ALL_TYPES.len();
        if self.type_filter.len() == all_count {
            // All selected → clear filter
            self.type_filter.clear();
            self.type_filter_cursor = 0;
            self.log(LogCategory::System, "Queue filter: all types".into());
        } else {
            let t = ALL_TYPES[self.type_filter_cursor % all_count].to_string();
            if self.type_filter.contains(&t) {
                self.type_filter.remove(&t);
            } else {
                self.type_filter.insert(t.clone());
            }
            self.type_filter_cursor = (self.type_filter_cursor + 1) % all_count;
            if self.type_filter.is_empty() {
                self.log(LogCategory::System, "Queue filter: all types".into());
            } else {
                let mut types: Vec<&str> = self
                    .type_filter
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                types.sort_unstable();
                self.log(
                    LogCategory::System,
                    format!("Queue filter: {}", types.join(",")),
                );
            }
        }
    }

    /// Cycle through event log category filters:
    /// None → System → Incoming → Deploy → Complete → Alert → Poll → None
    pub fn cycle_log_category_filter(&mut self) {
        use LogCategory::*;
        self.log_category_filter = match self.log_category_filter {
            None => Some(System),
            Some(System) => Some(Incoming),
            Some(Incoming) => Some(Deploy),
            Some(Deploy) => Some(Complete),
            Some(Complete) => Some(Alert),
            Some(Alert) => Some(Poll),
            Some(Poll) => None,
        };
        self.log_scroll = 0;
        let label = match self.log_category_filter {
            Some(cat) => cat.label().to_string(),
            None => "ALL".to_string(),
        };
        self.log(LogCategory::System, format!("Event log filter: {}", label));
    }

    /// Cycle to the next agent status filter. Resets selection to the first
    /// visible agent (or None if no agents match).
    pub fn cycle_agent_status_filter(&mut self) {
        self.agent_status_filter = self.agent_status_filter.next();
        self.log(
            LogCategory::System,
            format!("Agent filter: {}", self.agent_status_filter.label()),
        );
        // Reset selection to first visible agent
        let visible = self
            .agents
            .iter()
            .filter(|a| self.agent_status_filter.matches(a.status))
            .count();
        if visible == 0 {
            self.agent_list_state.select(None);
        } else {
            self.agent_list_state.select(Some(0));
        }
    }

    /// Return agents visible under the current agent status filter.
    /// Returns a Vec of references with their original index in `self.agents`.
    pub fn filtered_agents(&self) -> Vec<(usize, &AgentInstance)> {
        self.agents
            .iter()
            .enumerate()
            .filter(|(_, a)| self.agent_status_filter.matches(a.status))
            .collect()
    }

    /// Return the filtered view of `ready_tasks` according to current filters.
    /// The returned slice preserves the already-sorted order.
    pub fn filtered_tasks(&self) -> Vec<&BeadTask> {
        self.ready_tasks
            .iter()
            .filter(|t| {
                // Type filter
                if !self.type_filter.is_empty() {
                    let itype = t.issue_type.as_deref().unwrap_or("task");
                    if !self.type_filter.contains(itype) {
                        return false;
                    }
                }
                // Priority filter
                if let Some(ref range) = self.priority_filter {
                    let p = t.priority.unwrap_or(3);
                    if !range.contains(&p) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    pub fn on_agent_output(&mut self, agent_id: usize, line: String) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            if agent.status == AgentStatus::Starting {
                agent.status = AgentStatus::Running;
            }
            agent.output.push_back(line);
            if agent.output.len() > 10000 {
                agent.output.pop_front();
            }
        }
        self.lines_this_tick = self.lines_this_tick.saturating_add(1);
    }

    pub fn on_agent_exited(&mut self, agent_id: usize, exit_code: Option<i32>) {
        // Auto-detach interactive mode if this agent was the one we were attached to
        if self.interactive_mode && self.selected_agent_id == Some(agent_id) {
            self.interactive_mode = false;
        }

        // Clean up merge queue if this agent was queued/merging
        if let Some(pos) = self.merge_queue.iter().position(|e| e.agent_id == agent_id) {
            let entry = self.merge_queue.remove(pos).unwrap();
            self.log(
                LogCategory::Alert,
                format!(
                    "MERGE-QUEUE: AGENT-{:02} exited while merging {} (lock released, {} remaining)",
                    entry.unit_number, entry.task_id, self.merge_queue.len()
                ),
            );
        }

        let log_info = if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.exit_code = exit_code;
            agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            let unit = agent.unit_number;
            let task_id = agent.task.id.clone();
            let title = agent.task.title.clone();
            let rt = agent.runtime.name().to_string();
            let elapsed = agent.elapsed_secs;
            // Don't overwrite a terminal status — force_complete or kill may
            // have already set Completed/Failed before the exit watcher fires.
            if matches!(agent.status, AgentStatus::Completed | AgentStatus::Failed) {
                // If already completed via issue polling but usage wasn't read yet,
                // try reading it now (the process has fully exited so logs are flushed)
                if agent.usage.is_none() && agent.runtime == Runtime::ClaudeCode {
                    let ended_at = chrono::Utc::now();
                    agent.usage = crate::cost::read_agent_usage(agent.started_at_utc, ended_at);
                }
                None
            } else if exit_code == Some(0) {
                agent.status = AgentStatus::Completed;
                // Read Claude Code usage logs on successful exit
                if agent.runtime == Runtime::ClaudeCode {
                    let ended_at = chrono::Utc::now();
                    agent.usage = crate::cost::read_agent_usage(agent.started_at_utc, ended_at);
                }
                Some((true, unit, task_id, title, rt, elapsed))
            } else {
                agent.status = AgentStatus::Failed;
                // Read usage even on failure — the data is still valuable
                if agent.runtime == Runtime::ClaudeCode {
                    let ended_at = chrono::Utc::now();
                    agent.usage = crate::cost::read_agent_usage(agent.started_at_utc, ended_at);
                }
                Some((false, unit, task_id, title, rt, elapsed))
            }
        } else {
            None
        };

        // Record completion for the feed
        if let Some((success, _unit, ref task_id, _, ref rt, elapsed)) = log_info {
            if let Some(agent) = self.agents.iter().find(|a| a.task.id == *task_id) {
                let record = CompletionRecord {
                    task_id: task_id.clone(),
                    title: agent.task.title.clone(),
                    runtime: rt.clone(),
                    model: agent.model.clone(),
                    elapsed_secs: elapsed,
                    success,
                                    };
                self.recent_completions.push_back(record);
                if self.recent_completions.len() > 10 {
                    self.recent_completions.pop_front();
                }
            }
        }

        // Persist final PTY log to disk on process exit — always, even if the
        // agent was already marked Completed from issue closure (obelisk-3t3).
        self.persist_agent_pty_log(agent_id);

        if let Some((success, unit, task_id, title, rt, elapsed)) = log_info {
            if success {
                self.total_completed += 1;
                info!(
                    agent_id = unit,
                    task_id,
                    runtime = rt,
                    elapsed_secs = elapsed,
                    total_completed = self.total_completed,
                    event = "task_completed",
                    "beads task completed successfully"
                );
                self.log(
                    LogCategory::Complete,
                    format!("AGENT-{:02} completed {} [{}]", unit, task_id, rt),
                );
                if self.notifications_enabled {
                    crate::notify::send_notification(
                        "Agent Completed",
                        &format!("AGENT-{:02} \u{00b7} {} \u{00b7} {}s elapsed", unit, task_id, elapsed),
                    );
                    crate::notify::send_bell();
                    crate::notify::send_webhook(
                        &self.webhook_config,
                        WebhookEventType::AgentCompleted,
                        WebhookPayload {
                            event: "agent_completed".into(),
                            issue_id: task_id.clone(),
                            title,
                            status: "completed".into(),
                            runtime: Some(rt.clone()),
                            elapsed_secs: Some(elapsed),
                            exit_code: None,
                            failure_details: None,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            } else {
                self.total_failed += 1;
                warn!(
                    agent_id = unit,
                    task_id,
                    runtime = rt,
                    elapsed_secs = elapsed,
                    ?exit_code,
                    total_failed = self.total_failed,
                    event = "task_failed",
                    "beads task failed"
                );
                self.log(
                    LogCategory::Alert,
                    format!(
                        "AGENT-{:02} FAILED on {} [exit: {:?}]",
                        unit, task_id, exit_code
                    ),
                );
                if self.notifications_enabled {
                    // Collect failure context from the agent's last output lines
                    let failure_details = self
                        .agents
                        .iter()
                        .find(|a| a.task.id == task_id)
                        .map(|a| {
                            let lines: Vec<&str> = a.output.iter().map(|s| s.as_str()).collect();
                            let errors = detect_error_patterns(&lines);
                            if errors.is_empty() {
                                format!("exit code: {:?}", exit_code)
                            } else {
                                errors.join("; ")
                            }
                        });
                    crate::notify::send_notification(
                        "Agent Failed",
                        &format!("AGENT-{:02} \u{00b7} {} failed [exit: {:?}]", unit, task_id, exit_code),
                    );
                    crate::notify::send_bell();
                    crate::notify::send_webhook(
                        &self.webhook_config,
                        WebhookEventType::AgentFailed,
                        WebhookPayload {
                            event: "agent_failed".into(),
                            issue_id: task_id.clone(),
                            title,
                            status: "failed".into(),
                            runtime: Some(rt.clone()),
                            elapsed_secs: Some(elapsed),
                            exit_code,
                            failure_details,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            }
        }
    }

    /// Handle notification that an agent's beads issue has been polled and found closed.
    /// Transitions the agent to Done phase and marks it Completed without killing the
    /// process — the terminal stays open for inspection.
    pub fn on_issue_closed(&mut self, agent_id: usize) {
        let completion = if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            // Only act if the agent is still active
            if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
                return;
            }
            agent.phase = AgentPhase::Done;
            agent.status = AgentStatus::Completed;
            agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            // Read Claude Code usage logs for this agent (only for ClaudeCode runtime)
            if agent.runtime == Runtime::ClaudeCode {
                let ended_at = chrono::Utc::now();
                agent.usage = crate::cost::read_agent_usage(agent.started_at_utc, ended_at);
            }
            Some((
                agent.unit_number,
                agent.task.id.clone(),
                agent.task.title.clone(),
                agent.runtime.name().to_string(),
                agent.model.clone(),
                agent.elapsed_secs,
            ))
        } else {
            return;
        };

        // Dequeue from merge queue if still present (e.g. fast close)
        if let Some(pos) = self.merge_queue.iter().position(|e| e.agent_id == agent_id) {
            let entry = self.merge_queue.remove(pos).unwrap();
            let elapsed = entry.enqueued_at.elapsed().as_secs();
            self.log(
                LogCategory::System,
                format!(
                    "MERGE-QUEUE: AGENT-{:02} merge complete for {} ({}s in queue, {} remaining)",
                    entry.unit_number, entry.task_id, elapsed, self.merge_queue.len()
                ),
            );
        }

        if let Some((unit, task_id, title, rt, model, elapsed)) = completion {
            self.total_completed += 1;
            let record = CompletionRecord {
                task_id: task_id.clone(),
                title: title.clone(),
                runtime: rt.clone(),
                model,
                elapsed_secs: elapsed,
                success: true,
            };
            self.recent_completions.push_back(record);
            if self.recent_completions.len() > 10 {
                self.recent_completions.pop_front();
            }
            info!(
                agent_id = unit,
                task_id,
                runtime = rt,
                elapsed_secs = elapsed,
                total_completed = self.total_completed,
                event = "issue_closed",
                "beads issue closed — agent completed"
            );
            self.log(
                LogCategory::Complete,
                format!("AGENT-{:02} completed {} [{}] (issue closed)", unit, task_id, rt),
            );
            if self.notifications_enabled {
                crate::notify::send_notification(
                    "Agent Completed",
                    &format!(
                        "AGENT-{:02} \u{00b7} {} \u{00b7} {}s elapsed (issue closed)",
                        unit, task_id, elapsed
                    ),
                );
                crate::notify::send_bell();
                crate::notify::send_webhook(
                    &self.webhook_config,
                    WebhookEventType::AgentCompleted,
                    WebhookPayload {
                        event: "agent_completed".into(),
                        issue_id: task_id.clone(),
                        title,
                        status: "completed".into(),
                        runtime: Some(rt.clone()),
                        elapsed_secs: Some(elapsed),
                        exit_code: None,
                        failure_details: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    },
                );
            }
        }
    }

    pub fn selected_task(&self) -> Option<&BeadTask> {
        self.task_list_state
            .selected()
            .and_then(|i| self.filtered_tasks().into_iter().nth(i))
    }

    pub fn get_spawn_info(&mut self) -> Option<SpawnRequest> {
        if self.active_agent_count() >= self.max_concurrent {
            self.log(
                LogCategory::Alert,
                "Max concurrent agents reached — cannot deploy".into(),
            );
            return None;
        }

        let task = self.selected_task()?.clone();
        if task.is_epic() {
            self.log(
                LogCategory::Alert,
                format!("Cannot spawn agent for epic {} — epics are containers, work their children instead", task.id),
            );
            return None;
        }
        let runtime = self.selected_runtime;

        let model = self.selected_model_for(runtime).to_string();
        let unit = self.next_unit;
        self.next_unit += 1;

        let issue_type = task.issue_type.as_deref().unwrap_or("task");
        let resolved = templates::resolve(&self.template_dir, issue_type);
        let system_prompt = templates::interpolate(
            &resolved.content,
            &task.id,
            &task.title,
            task.priority,
            task.description.as_deref(),
        );
        let template_name = resolved.name.clone();
        let user_prompt = format!(
            "Work on beads issue {}. Follow the workflow in the Beads Agent Prompt exactly.",
            task.id
        );

        let agent = AgentInstance {
            id: unit,
            unit_number: unit,
            task: task.clone(),
            runtime,
            model: model.clone(),
            status: AgentStatus::Starting,
            phase: AgentPhase::Detecting,
            output: VecDeque::new(),
            started_at: std::time::Instant::now(),
            elapsed_secs: 0,
            exit_code: None,
            pid: None,
            retry_count: 0,
            worktree_path: Some(format!("../worktree-{}", task.id)),
            worktree_cleaned: false,
            pinned_to_split: None,
            template_name: template_name.clone(),
            total_lines: 0,
            raw_pty_log: Vec::new(),
            pty_log_flushed_bytes: 0,
            started_at_utc: chrono::Utc::now(),
            usage: None,
        };

        self.claimed_task_ids.insert(task.id.clone());
        self.agents.push(agent);
        info!(
            agent_id = unit,
            task_id = %task.id,
            task_title = %task.title,
            runtime = %runtime.name(),
            model,
            template = %template_name,
            event = "agent_deployed",
            "agent deployed for task"
        );
        self.log(
            LogCategory::Deploy,
            format!(
                "AGENT-{:02} deployed on {} [{}/{}] tmpl={}",
                unit,
                task.id,
                runtime.name(),
                model,
                template_name,
            ),
        );

        self.ready_tasks.retain(|t| t.id != task.id);
        let filtered_len = self.filtered_tasks().len();
        if let Some(sel) = self.task_list_state.selected() {
            if sel >= filtered_len && filtered_len > 0 {
                self.task_list_state.select(Some(filtered_len - 1));
            } else if filtered_len == 0 {
                self.task_list_state.select(None);
            }
        }

        Some(SpawnRequest {
            task,
            runtime,
            model,
            agent_id: unit,
            system_prompt,
            user_prompt,
            pty_rows: self.last_pty_size.0,
            pty_cols: self.last_pty_size.1,
        })
    }

    pub fn get_auto_spawn_info(&mut self) -> Option<SpawnRequest> {
        if !self.auto_spawn || self.active_agent_count() >= self.max_concurrent {
            return None;
        }

        // Build set of task IDs that are explicitly blocked
        let blocked_ids: HashSet<String> = self
            .blocked_tasks
            .iter()
            .map(|bt| bt.task.id.clone())
            .collect();

        // Build set of task IDs whose dep-graph parents are not yet closed.
        // In the dep tree, parent_id represents a dependency — if a task's parent
        // isn't closed, the task should not be spawned.
        let status_map: HashMap<&str, &str> = self
            .dep_graph_nodes
            .iter()
            .map(|n| (n.id.as_str(), n.status.as_str()))
            .collect();
        let dep_blocked: HashSet<String> = self
            .dep_graph_nodes
            .iter()
            .filter(|node| {
                node.parent_id.as_ref().is_some_and(|pid| {
                    status_map
                        .get(pid.as_str())
                        .is_some_and(|s| *s != "closed")
                })
            })
            .map(|n| n.id.clone())
            .collect();

        // Respect filters — only auto-spawn from the visible filtered set.
        // Always pick the highest-priority task (lowest priority number)
        // that has all dependencies closed.
        let filtered = self.filtered_tasks();
        let best_idx = filtered
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_epic())
            .filter(|(_, t)| !blocked_ids.contains(&t.id) && !dep_blocked.contains(&t.id))
            .min_by_key(|(_, t)| t.priority.unwrap_or(3))
            .map(|(i, _)| i);

        let best_idx = match best_idx {
            Some(idx) => idx,
            None => {
                // All tasks have unclosed deps — log and skip
                if !filtered.is_empty() {
                    let skipped: Vec<&str> = filtered
                        .iter()
                        .filter(|t| blocked_ids.contains(&t.id) || dep_blocked.contains(&t.id))
                        .map(|t| t.id.as_str())
                        .collect();
                    if !skipped.is_empty() {
                        info!(
                            skipped_ids = %skipped.join(","),
                            event = "auto_spawn_deps_blocked",
                            "skipping tasks with unclosed dependencies"
                        );
                    }
                }
                return None;
            }
        };

        self.task_list_state.select(Some(best_idx));
        self.get_spawn_info()
    }

    pub fn navigate_up(&mut self) {
        match self.active_view {
            View::Dashboard => match self.focus {
                Focus::ReadyQueue => {
                    let len = self.filtered_tasks().len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .task_list_state
                        .selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    self.task_list_state.select(Some(i));
                }
                Focus::BlockedQueue => {
                    let len = self.blocked_tasks.len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .blocked_list_state
                        .selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    self.blocked_list_state.select(Some(i));
                }
                Focus::AgentList => {
                    let len = self.filtered_agents().len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .agent_list_state
                        .selected()
                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                        .unwrap_or(0);
                    self.agent_list_state.select(Some(i));
                }
            },
            View::AgentDetail => {
                if let Some(agent_id) = self.selected_agent_id {
                    if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
                        let total = agent.output.len();
                        match self.agent_output_scroll {
                            None => {
                                // Auto-follow → switch to manual one line up from bottom
                                if total > 0 {
                                    self.agent_output_scroll = Some(total.saturating_sub(1));
                                }
                            }
                            Some(pos) => {
                                if pos > 0 {
                                    self.agent_output_scroll = Some(pos - 1);
                                }
                            }
                        }
                    }
                }
            }
            View::EventLog => {
                if self.log_scroll > 0 {
                    self.log_scroll -= 1;
                }
            }
            View::History => {
                self.history_scroll = self.history_scroll.saturating_sub(1);
            }
            View::SplitPane => {
                self.split_pane_scroll_up();
            }
            View::WorktreeOverview => {
                let len = self.worktree_entries.len();
                if len == 0 { return; }
                let i = self.worktree_list_state.selected()
                    .map(|i| if i == 0 { len - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.worktree_list_state.select(Some(i));
            }
            View::DepGraph => {
                let len = self.dep_graph_rows.len();
                if len == 0 { return; }
                let i = self.dep_graph_list_state.selected()
                    .map(|i| if i == 0 { len - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.dep_graph_list_state.select(Some(i));
            }
        }
    }

    pub fn navigate_down(&mut self) {
        match self.active_view {
            View::Dashboard => match self.focus {
                Focus::ReadyQueue => {
                    let len = self.filtered_tasks().len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .task_list_state
                        .selected()
                        .map(|i| if i + 1 >= len { 0 } else { i + 1 })
                        .unwrap_or(0);
                    self.task_list_state.select(Some(i));
                }
                Focus::BlockedQueue => {
                    let len = self.blocked_tasks.len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .blocked_list_state
                        .selected()
                        .map(|i| if i + 1 >= len { 0 } else { i + 1 })
                        .unwrap_or(0);
                    self.blocked_list_state.select(Some(i));
                }
                Focus::AgentList => {
                    let len = self.filtered_agents().len();
                    if len == 0 {
                        return;
                    }
                    let i = self
                        .agent_list_state
                        .selected()
                        .map(|i| if i + 1 >= len { 0 } else { i + 1 })
                        .unwrap_or(0);
                    self.agent_list_state.select(Some(i));
                }
            },
            View::AgentDetail => {
                if let Some(agent_id) = self.selected_agent_id {
                    if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
                        let total = agent.output.len();
                        if let Some(pos) = self.agent_output_scroll {
                            if pos + 1 >= total {
                                // Reached bottom → re-engage auto-follow
                                self.agent_output_scroll = None;
                            } else {
                                self.agent_output_scroll = Some(pos + 1);
                            }
                        }
                        // None (auto-follow) + Down → stay at auto-follow
                    }
                }
            }
            View::EventLog => {
                let filtered_len = match self.log_category_filter {
                    Some(cat) => self.event_log.iter().filter(|e| e.category == cat).count(),
                    None => self.event_log.len(),
                };
                if self.log_scroll < filtered_len {
                    self.log_scroll += 1;
                }
            }
            View::History => {
                let max = self.history_sessions.len().saturating_sub(1);
                if self.history_scroll < max {
                    self.history_scroll += 1;
                }
            }
            View::SplitPane => {
                self.split_pane_scroll_down();
            }
            View::WorktreeOverview => {
                let len = self.worktree_entries.len();
                if len == 0 { return; }
                let i = self.worktree_list_state.selected()
                    .map(|i| if i + 1 >= len { 0 } else { i + 1 })
                    .unwrap_or(0);
                self.worktree_list_state.select(Some(i));
            }
            View::DepGraph => {
                let len = self.dep_graph_rows.len();
                if len == 0 { return; }
                let i = self.dep_graph_list_state.selected()
                    .map(|i| if i + 1 >= len { 0 } else { i + 1 })
                    .unwrap_or(0);
                self.dep_graph_list_state.select(Some(i));
            }
        }
    }

    pub fn page_up(&mut self) {
        if let Some(agent_id) = self.selected_agent_id {
            if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
                let total = agent.output.len();
                let page = 20;
                match self.agent_output_scroll {
                    None => {
                        self.agent_output_scroll = Some(total.saturating_sub(page));
                    }
                    Some(pos) => {
                        self.agent_output_scroll = Some(pos.saturating_sub(page));
                    }
                }
            }
        }
    }

    pub fn page_down(&mut self) {
        if let Some(agent_id) = self.selected_agent_id {
            if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
                let total = agent.output.len();
                let page = 20;
                if let Some(pos) = self.agent_output_scroll {
                    if pos + page >= total {
                        self.agent_output_scroll = None;
                    } else {
                        self.agent_output_scroll = Some(pos + page);
                    }
                }
            }
        }
    }

    pub fn toggle_focus(&mut self) {
        if self.active_view == View::Dashboard {
            self.focus = match self.focus {
                Focus::ReadyQueue => Focus::BlockedQueue,
                Focus::BlockedQueue => Focus::AgentList,
                Focus::AgentList => {
                    // Reset agent status filter when leaving the agent panel
                    self.agent_status_filter = AgentStatusFilter::All;
                    Focus::ReadyQueue
                }
            };
        }
    }

    pub fn enter_pressed(&mut self) {
        if self.active_view == View::Dashboard && self.focus == Focus::AgentList {
            if let Some(sel) = self.agent_list_state.selected() {
                let visible = self.filtered_agents();
                if sel < visible.len() {
                    self.selected_agent_id = Some(visible[sel].1.id);
                    self.agent_output_scroll = None;
                    self.active_view = View::AgentDetail;
                }
            }
        }
    }

    pub fn on_agent_pid(&mut self, agent_id: usize, pid: u32) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.pid = Some(pid);
        }
    }

    /// Send SIGTERM to a process, with PID validation and error checking.
    /// Returns true if the signal was sent successfully (or on non-unix), false otherwise.
    #[allow(unused_variables)]
    fn send_sigterm(pid: u32, context: &str) -> bool {
        #[cfg(unix)]
        {
            let Ok(pid_i32) = i32::try_from(pid) else {
                error!(pid, context, "PID overflows i32, cannot send signal");
                return false;
            };
            if pid_i32 <= 0 {
                error!(pid_i32, context, "invalid PID (must be > 0), refusing to signal");
                return false;
            }
            let ret = unsafe { libc::kill(pid_i32, libc::SIGTERM) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                error!(pid_i32, %err, context, "libc::kill(SIGTERM) failed");
                return false;
            }
            true
        }
        #[cfg(windows)]
        {
            use std::process::Command;
            let status = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match status {
                Ok(s) if s.success() => true,
                Ok(s) => {
                    error!(pid, ?s, context, "taskkill failed");
                    false
                }
                Err(e) => {
                    error!(pid, %e, context, "taskkill could not be launched");
                    false
                }
            }
        }
    }

    /// Kill agent process via SIGTERM. Returns (unit_number, worktree_path) for logging/cleanup.
    pub fn kill_agent(&mut self, agent_id: usize) -> Option<(usize, Option<String>)> {
        let agent = self.agents.iter_mut().find(|a| a.id == agent_id)?;
        if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
            return None;
        }
        if let Some(pid) = agent.pid {
            Self::send_sigterm(pid, "kill_agent");
        }
        agent.status = AgentStatus::Failed;
        agent.elapsed_secs = agent.started_at.elapsed().as_secs();
        let unit = agent.unit_number;
        let task_id = agent.task.id.clone();
        let worktree = agent.worktree_path.clone();
        self.total_failed += 1;
        warn!(
            agent_id = unit,
            task_id,
            elapsed_secs = agent.elapsed_secs,
            event = "agent_killed",
            "agent terminated by user"
        );
        self.log(
            LogCategory::Alert,
            format!("AGENT-{:02} terminated (killed)", unit),
        );
        Some((unit, worktree))
    }

    /// Mark an active agent as completed and send SIGTERM to clean up.
    /// Returns (unit_number, worktree_path) for logging/cleanup.
    pub fn force_complete_agent(&mut self, agent_id: usize) -> Option<(usize, Option<String>)> {
        let agent = self.agents.iter_mut().find(|a| a.id == agent_id)?;
        if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
            return None;
        }
        if let Some(pid) = agent.pid {
            Self::send_sigterm(pid, "force_complete_agent");
        }
        agent.status = AgentStatus::Completed;
        agent.elapsed_secs = agent.started_at.elapsed().as_secs();
        let unit = agent.unit_number;
        let task_id = agent.task.id.clone();
        let worktree = agent.worktree_path.clone();
        self.total_completed += 1;
        info!(
            agent_id = unit,
            task_id,
            elapsed_secs = agent.elapsed_secs,
            event = "agent_force_completed",
            "agent manually marked as completed"
        );
        self.log(
            LogCategory::Complete,
            format!("AGENT-{:02} marked complete (manual)", unit),
        );
        Some((unit, worktree))
    }

    /// Dismiss (remove) the currently selected agent if it is completed or failed.
    /// Returns Some(message) describing the outcome; None if no agent was selected.
    pub fn dismiss_selected_agent(&mut self) -> Option<String> {
        let sel = self.agent_list_state.selected()?;
        // Map filtered-list index → raw index in self.agents
        let raw_idx = {
            let visible = self.filtered_agents();
            if sel >= visible.len() {
                return None;
            }
            visible[sel].0
        };
        let agent = &self.agents[raw_idx];
        if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
            return Some(format!(
                "AGENT-{:02} is active — cannot dismiss",
                agent.unit_number
            ));
        }
        let agent_id = agent.id;
        let unit = agent.unit_number;
        self.agents.remove(raw_idx);
        self.pty_states.remove(&agent_id);
        if self.selected_agent_id == Some(agent_id) {
            self.selected_agent_id = None;
        }
        // Recompute visible count after removal
        let new_visible = self.filtered_agents().len();
        if new_visible == 0 {
            self.agent_list_state.select(None);
        } else if sel >= new_visible {
            self.agent_list_state.select(Some(new_visible - 1));
        }
        Some(format!("AGENT-{:02} dismissed", unit))
    }

    /// Dismiss all completed and failed agents at once. Returns the count removed.
    pub fn dismiss_all_finished(&mut self) -> usize {
        let finished_ids: Vec<usize> = self
            .agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Completed | AgentStatus::Failed))
            .map(|a| a.id)
            .collect();
        let count = finished_ids.len();
        for id in &finished_ids {
            self.pty_states.remove(id);
        }
        if self
            .selected_agent_id
            .map(|id| finished_ids.contains(&id))
            .unwrap_or(false)
        {
            self.selected_agent_id = None;
        }
        self.agents.retain(|a| !finished_ids.contains(&a.id));
        // Re-check against filtered view (since finished agents may be the ones hidden by filter)
        let new_visible = self.filtered_agents().len();
        if new_visible == 0 {
            self.agent_list_state.select(None);
        } else if let Some(sel) = self.agent_list_state.selected() {
            if sel >= new_visible {
                self.agent_list_state.select(Some(new_visible - 1));
            }
        }
        count
    }

    /// Kill all running agent processes (called on shutdown).
    pub fn kill_all_agents(&mut self) {
        let active_ids: Vec<usize> = self
            .agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .map(|a| a.id)
            .collect();
        for id in active_ids {
            self.kill_agent(id);
        }
    }

    /// Retry a failed agent: create a new AgentInstance for the same task.
    /// Returns None if the agent is not failed, max retries exceeded, or at capacity.
    pub fn retry_agent(&mut self, agent_id: usize) -> Option<SpawnRequest> {
        let (task, runtime, model, retry_count, failure_context) = {
            let agent = self.agents.iter().find(|a| a.id == agent_id)?;
            if agent.status != AgentStatus::Failed {
                return None;
            }
            if agent.retry_count >= self.max_retries {
                self.log(
                    LogCategory::Alert,
                    format!(
                        "AGENT-{:02} reached max retries ({}) — not retrying",
                        agent.unit_number, self.max_retries
                    ),
                );
                return None;
            }
            let ctx = extract_failure_context(agent, self.retry_context_lines);
            (agent.task.clone(), agent.runtime, agent.model.clone(), agent.retry_count, ctx)
        };

        if self.active_agent_count() >= self.max_concurrent {
            self.log(LogCategory::Alert, "Max concurrent agents reached — cannot retry".into());
            return None;
        }

        let unit = self.next_unit;
        self.next_unit += 1;
        let new_retry_count = retry_count + 1;

        let issue_type = task.issue_type.as_deref().unwrap_or("task");
        let resolved = templates::resolve(&self.template_dir, issue_type);
        let system_prompt = templates::interpolate(
            &resolved.content,
            &task.id,
            &task.title,
            task.priority,
            task.description.as_deref(),
        );
        let template_name = resolved.name.clone();
        let user_prompt = format!(
            "Work on beads issue {}. Follow the workflow in the Beads Agent Prompt exactly.\n\n\
             ---\n\n\
             # RETRY CONTEXT (Attempt #{} — previous attempt failed)\n\n\
             PREVIOUS ATTEMPT FAILED. Review the failure context below and avoid \
             repeating the same mistakes. Adjust your approach based on what went wrong.\n\n\
             {}\n\n\
             ---",
            task.id, new_retry_count, failure_context
        );

        let new_agent = AgentInstance {
            id: unit,
            unit_number: unit,
            task: task.clone(),
            runtime,
            model: model.clone(),
            status: AgentStatus::Starting,
            phase: AgentPhase::Detecting,
            output: VecDeque::new(),
            started_at: std::time::Instant::now(),
            elapsed_secs: 0,
            exit_code: None,
            pid: None,
            retry_count: new_retry_count,
            worktree_path: Some(format!("../worktree-{}", task.id)),
            worktree_cleaned: false,
            pinned_to_split: None,
            template_name: template_name.clone(),
            total_lines: 0,
            raw_pty_log: Vec::new(),
            pty_log_flushed_bytes: 0,
            started_at_utc: chrono::Utc::now(),
            usage: None,
        };

        // task ID remains in claimed_task_ids (the new agent owns it)
        self.agents.push(new_agent);
        self.selected_agent_id = Some(unit);
        self.agent_output_scroll = None;

        info!(
            agent_id = unit,
            task_id = %task.id,
            runtime = %runtime.name(),
            model,
            retry_count = new_retry_count,
            template = %template_name,
            event = "agent_retry",
            "retrying failed agent"
        );
        self.log(
            LogCategory::Deploy,
            format!(
                "AGENT-{:02} RETRY #{} on {} [{}/{}] tmpl={}",
                unit, new_retry_count, task.id, runtime.name(), model, template_name,
            ),
        );

        Some(SpawnRequest {
            task,
            runtime,
            model,
            agent_id: unit,
            system_prompt,
            user_prompt,
            pty_rows: self.last_pty_size.0,
            pty_cols: self.last_pty_size.1,
        })
    }

    /// Returns the set of task IDs that have actively running agents.
    /// Used to distinguish orphaned worktrees from in-use ones.
    pub fn active_task_ids(&self) -> std::collections::HashSet<String> {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .map(|a| a.task.id.clone())
            .collect()
    }

    /// Log warnings about orphaned worktrees found on startup and show an alert.
    pub fn on_worktree_orphans(&mut self, paths: Vec<String>) {
        for path in &paths {
            self.log(
                LogCategory::Alert,
                format!("Orphaned worktree: {}  (press 'c' on dashboard to clean up)", path),
            );
        }
        if !paths.is_empty() {
            self.alert_message = Some((
                format!(
                    "{} ORPHANED WORKTREE(S) — press 'c' to clean up",
                    paths.len()
                ),
                self.frame_count + 100,
            ));
        }
    }

    /// Update agent state and log after a worktree cleanup operation.
    pub fn on_worktree_cleaned(&mut self, cleaned: Vec<String>, failed: Vec<String>) {
        for path in &cleaned {
            // Mark the corresponding agent as cleaned up
            if let Some(agent) = self
                .agents
                .iter_mut()
                .find(|a| a.worktree_path.as_deref() == Some(path.as_str()))
            {
                agent.worktree_cleaned = true;
            }
            self.log(LogCategory::System, format!("Worktree cleaned: {}", path));
        }
        for path in &failed {
            self.log(
                LogCategory::Alert,
                format!("Worktree cleanup failed: {}", path),
            );
        }
        if !cleaned.is_empty() {
            self.alert_message = Some((
                format!("{} WORKTREE(S) CLEANED UP", cleaned.len()),
                self.frame_count + 50,
            ));
        }
    }

    /// Process worktree scan results and build enriched entries for the overview panel.
    pub fn on_worktree_scanned(&mut self, worktrees: Vec<(String, String)>) {
        let active_ids = self.active_task_ids();

        let mut entries: Vec<WorktreeEntry> = worktrees
            .into_iter()
            .map(|(path, branch)| {
                // Parse issue ID from worktree-{id} naming
                let issue_id = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_prefix("worktree-"))
                    .map(|s| s.to_string());

                // Find associated agent
                let agent_id = self
                    .agents
                    .iter()
                    .find(|a| a.worktree_path.as_deref() == Some(path.as_str())
                        || issue_id.as_deref().map(|id| a.task.id == id).unwrap_or(false))
                    .map(|a| a.id);

                // Classify status
                let status = if let Some(aid) = agent_id {
                    if self.agents.iter().any(|a| a.id == aid && matches!(a.status, AgentStatus::Starting | AgentStatus::Running)) {
                        WorktreeStatus::Active
                    } else {
                        WorktreeStatus::Idle
                    }
                } else if issue_id.as_deref().map(|id| active_ids.contains(id)).unwrap_or(false) {
                    WorktreeStatus::Active
                } else {
                    WorktreeStatus::Orphaned
                };

                // Get creation time from filesystem
                let created_at = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.created().ok())
                    .map(chrono::DateTime::<chrono::Local>::from);

                WorktreeEntry {
                    path,
                    branch,
                    issue_id,
                    agent_id,
                    status,
                    created_at,
                }
            })
            .collect();

        // Sort based on current sort mode
        self.sort_worktree_entries(&mut entries);
        self.worktree_entries = entries;
    }

    fn sort_worktree_entries(&self, entries: &mut [WorktreeEntry]) {
        match self.worktree_sort_mode {
            WorktreeSortMode::Age => {
                entries.sort_by(|a, b| {
                    let a_time = a.created_at.as_ref().map(|t| t.timestamp()).unwrap_or(0);
                    let b_time = b.created_at.as_ref().map(|t| t.timestamp()).unwrap_or(0);
                    b_time.cmp(&a_time) // newest first
                });
            }
            WorktreeSortMode::Status => {
                entries.sort_by_key(|e| match e.status {
                    WorktreeStatus::Orphaned => 0, // orphaned first (most actionable)
                    WorktreeStatus::Active => 1,
                    WorktreeStatus::Idle => 2,
                });
            }
        }
    }

    pub fn cycle_worktree_sort(&mut self) {
        self.worktree_sort_mode = self.worktree_sort_mode.next();
        let mut entries = std::mem::take(&mut self.worktree_entries);
        self.sort_worktree_entries(&mut entries);
        self.worktree_entries = entries;
    }

    // ── Dependency Graph ──

    pub fn on_dep_graph_result(&mut self, nodes: Vec<DepNode>) {
        self.dep_graph_nodes = nodes;
        self.rebuild_dep_graph_rows();
    }

    pub fn on_dep_graph_failed(&mut self, error: String) {
        self.log(LogCategory::Poll, format!("Dep graph poll failed: {}", error));
    }

    /// Rebuild the flattened row list from dep_graph_nodes, respecting collapsed state.
    pub fn rebuild_dep_graph_rows(&mut self) {
        let nodes = &self.dep_graph_nodes;

        // Build parent→children map
        let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
        let mut roots: Vec<usize> = Vec::new();

        for (i, node) in nodes.iter().enumerate() {
            match &node.parent_id {
                Some(pid) if !pid.is_empty() => {
                    children_map.entry(pid.clone()).or_default().push(i);
                }
                _ => {
                    // depth 0 or no parent = root
                    if node.depth == 0 {
                        roots.push(i);
                    }
                }
            }
        }

        // If no tree structure detected (all depth 0, no parents), just show flat list
        if roots.is_empty() || (roots.len() == nodes.len() && children_map.is_empty()) {
            self.dep_graph_rows = nodes
                .iter()
                .map(|n| {
                    let has_children = children_map.contains_key(&n.id);
                    DepGraphRow {
                        node: n.clone(),
                        collapsed: self.dep_graph_collapsed.contains(&n.id),
                        has_children,
                    }
                })
                .collect();
            return;
        }

        // DFS to build flattened rows
        let mut rows = Vec::new();
        let mut stack: Vec<(usize, usize)> = Vec::new(); // (node_index, depth)

        // Sort roots by priority (highest first)
        let mut sorted_roots = roots;
        sorted_roots.sort_by(|a, b| {
            let pa = nodes[*a].priority.unwrap_or(99);
            let pb = nodes[*b].priority.unwrap_or(99);
            pa.cmp(&pb)
        });

        for &ri in sorted_roots.iter().rev() {
            stack.push((ri, 0));
        }

        while let Some((idx, depth)) = stack.pop() {
            let node = &nodes[idx];
            let has_children = children_map.contains_key(&node.id);
            let is_collapsed = self.dep_graph_collapsed.contains(&node.id);

            let mut display_node = node.clone();
            display_node.depth = depth;

            rows.push(DepGraphRow {
                node: display_node,
                collapsed: is_collapsed,
                has_children,
            });

            // If not collapsed, push children
            if !is_collapsed {
                if let Some(child_indices) = children_map.get(&node.id) {
                    // Push in reverse so first child is processed first
                    for &ci in child_indices.iter().rev() {
                        stack.push((ci, depth + 1));
                    }
                }
            }
        }

        self.dep_graph_rows = rows;
    }

    /// Toggle collapse/expand for the currently selected dep graph node.
    pub fn dep_graph_toggle_collapse(&mut self) {
        if let Some(sel) = self.dep_graph_list_state.selected() {
            if sel < self.dep_graph_rows.len() {
                let id = self.dep_graph_rows[sel].node.id.clone();
                if self.dep_graph_collapsed.contains(&id) {
                    self.dep_graph_collapsed.remove(&id);
                } else {
                    self.dep_graph_collapsed.insert(id);
                }
                self.rebuild_dep_graph_rows();
            }
        }
    }

    pub fn toggle_diff_panel(&mut self) {
        self.show_diff_panel = !self.show_diff_panel;
        if self.show_diff_panel {
            self.diff_scroll = 0;
            // Force an immediate diff poll by resetting the timer
            self.diff_last_poll_frame = 0;
        } else {
            self.diff_data = None;
        }
    }

    pub fn on_diff_result(&mut self, agent_id: usize, diff: DiffData) {
        // Only accept if we're still viewing this agent with diff panel open
        if self.show_diff_panel && self.selected_agent_id == Some(agent_id) {
            self.diff_data = Some(diff);
        }
    }

    /// Returns the worktree path for the currently selected agent, if it has one
    /// and the worktree hasn't been cleaned up.
    pub fn selected_agent_worktree(&self) -> Option<String> {
        self.selected_agent_id
            .and_then(|id| self.agents.iter().find(|a| a.id == id))
            .and_then(|a| {
                if a.worktree_cleaned {
                    None
                } else {
                    a.worktree_path.clone()
                }
            })
    }

    /// Returns context-appropriate text for the 'y' (yank) keybinding:
    /// - Ready queue: issue ID
    /// - Agent list: "AGENT-NN issue-id"
    /// - Agent detail: worktree path
    pub fn yank_text(&self) -> Option<String> {
        match self.active_view {
            View::Dashboard => match self.focus {
                Focus::ReadyQueue => {
                    self.selected_task().map(|t| t.id.clone())
                }
                Focus::BlockedQueue => {
                    self.blocked_list_state
                        .selected()
                        .and_then(|i| self.blocked_tasks.get(i))
                        .map(|bt| bt.task.id.clone())
                }
                Focus::AgentList => {
                    let filtered = self.filtered_agents();
                    self.agent_list_state
                        .selected()
                        .and_then(|i| filtered.get(i))
                        .map(|(_, agent)| {
                            format!("AGENT-{:02} {}", agent.unit_number, agent.task.id)
                        })
                }
            },
            View::AgentDetail => {
                self.selected_agent_worktree()
            }
            _ => None,
        }
    }

    pub fn selected_model(&self) -> &'static str {
        let idx = self.model_indices.get(&self.selected_runtime).copied().unwrap_or(0);
        let models = self.selected_runtime.models();
        models[idx % models.len()]
    }

    pub fn selected_model_for(&self, runtime: Runtime) -> &'static str {
        let idx = self.model_indices.get(&runtime).copied().unwrap_or(0);
        let models = runtime.models();
        models[idx % models.len()]
    }

    pub fn cycle_model(&mut self) {
        let models = self.selected_runtime.models();
        let idx = self.model_indices.entry(self.selected_runtime).or_insert(0);
        *idx = (*idx + 1) % models.len();
    }

    /// Recompute search matches for the selected agent's PTY screen.
    pub fn update_search_matches(&mut self) {
        let agent_id = match self.selected_agent_id {
            Some(id) => id,
            None => {
                self.search_matches.clear();
                return;
            }
        };
        let matches = if let Some(state) = self.pty_states.get(&agent_id) {
            compute_search_matches(state.parser.screen(), &self.search_query)
        } else {
            Vec::new()
        };
        self.search_matches = matches;
        if !self.search_matches.is_empty() && self.search_current_idx >= self.search_matches.len() {
            self.search_current_idx = 0;
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_current_idx = (self.search_current_idx + 1) % self.search_matches.len();
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        if self.search_current_idx == 0 {
            self.search_current_idx = self.search_matches.len() - 1;
        } else {
            self.search_current_idx -= 1;
        }
    }

    // ── Jump-to-issue helpers ──

    /// Collect matches for the current jump query across ready queue and agents.
    /// Returns vec of (label, is_agent) where label is the issue ID.
    pub fn jump_matches(&self) -> Vec<(String, bool)> {
        if self.jump_query.is_empty() {
            return Vec::new();
        }
        let q = self.jump_query.to_lowercase();
        let mut results = Vec::new();

        // Search ready queue
        for task in &self.ready_tasks {
            if task.id.to_lowercase().contains(&q) {
                results.push((task.id.clone(), false));
            }
        }

        // Search agents (by task ID)
        for agent in &self.agents {
            if agent.task.id.to_lowercase().contains(&q) {
                // Avoid duplicates if same ID appears in both
                let id = agent.task.id.clone();
                if !results.iter().any(|(existing, is_agent)| existing == &id && *is_agent) {
                    results.push((id, true));
                }
            }
        }

        results
    }

    /// Execute jump: find the best match and navigate to it.
    /// Returns true if a match was found.
    pub fn jump_execute(&mut self) -> bool {
        let q = self.jump_query.to_lowercase();
        if q.is_empty() {
            return false;
        }

        // First, try ready queue (filtered view)
        let filtered = self.filtered_tasks();
        for (i, task) in filtered.iter().enumerate() {
            if task.id.to_lowercase().contains(&q) {
                self.active_view = View::Dashboard;
                self.focus = Focus::ReadyQueue;
                self.task_list_state.select(Some(i));
                return true;
            }
        }

        // Then, try agents (filtered view)
        let agents = self.filtered_agents();
        for (i, (_, agent)) in agents.iter().enumerate() {
            if agent.task.id.to_lowercase().contains(&q) {
                self.active_view = View::Dashboard;
                self.focus = Focus::AgentList;
                self.agent_list_state.select(Some(i));
                return true;
            }
        }

        // Try unfiltered ready queue as fallback
        for (i, task) in self.ready_tasks.iter().enumerate() {
            if task.id.to_lowercase().contains(&q) {
                // Clear filters so the item is visible, then select
                self.type_filter.clear();
                self.priority_filter = None;
                self.active_view = View::Dashboard;
                self.focus = Focus::ReadyQueue;
                self.task_list_state.select(Some(i));
                return true;
            }
        }

        // Try unfiltered agents as fallback
        for (i, agent) in self.agents.iter().enumerate() {
            if agent.task.id.to_lowercase().contains(&q) {
                self.agent_status_filter = AgentStatusFilter::All;
                self.active_view = View::Dashboard;
                self.focus = Focus::AgentList;
                self.agent_list_state.select(Some(i));
                return true;
            }
        }

        false
    }

    /// Store PTY handle for an agent (called when AgentPtyReady arrives).
    pub fn on_agent_pty_ready(&mut self, agent_id: usize, handle: PtyHandle) {
        self.pty_states.insert(agent_id, handle);
    }

    /// Feed raw PTY bytes into the agent's vt100 parser.
    /// Also intercepts ESC[6n (Device Status Report) queries from the PTY and
    /// responds with a CPR so that ConPTY on Windows doesn't stall.
    pub fn on_agent_pty_data(&mut self, agent_id: usize, data: &[u8]) {
        if let Some(state) = self.pty_states.get_mut(&agent_id) {
            // Intercept DSR query (ESC[6n) from child process and reply with CPR.
            // ConPTY on Windows sends this on startup; without a response it buffers
            // all output indefinitely. On macOS/Linux the CPR response leaks back as
            // visible text (^[[1;1R), so only send it on Windows.
            if cfg!(target_os = "windows")
                && data.windows(4).any(|w| w == b"\x1b[6n") {
                    use std::io::Write;
                    let _ = state.writer.write_all(b"\x1b[1;1R");
                    let _ = state.writer.flush();
                }
            // Strip any CPR responses (ESC[row;colR) that leak into the output
            // stream before feeding the vt100 parser — defense-in-depth.
            let cleaned = RE_CPR.replace_all(data, &b""[..]);
            state.parser.process(&cleaned);
            // Track cumulative scrollback: only count positive deltas so that
            // screen clears / alternate-screen resets never reduce the total.
            let cur = state.parser.screen().scrollback();
            if cur > state.prev_scrollback {
                state.cumulative_scrollback += cur - state.prev_scrollback;
            }
            state.prev_scrollback = cur;
            // Track cumulative newlines — platform-independent fallback for line
            // counting.  On Windows ConPTY the vt100 parser never accumulates
            // scrollback because ConPTY uses cursor-positioning to repaint rather
            // than scroll sequences.  Counting raw '\n' bytes keeps the total
            // incrementing on every platform.
            let nl = data.iter().filter(|&&b| b == b'\n').count();
            state.cumulative_newlines += nl;
        }
        // Capture raw PTY bytes for log export
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.raw_pty_log.extend_from_slice(data);
        }
        // Transition Starting → Running on first data received, and detect phase.
        // Done phase detection is handled by polling `bd show` — see on_issue_closed().
        //
        // Merge queue events are collected here and processed outside the borrow scope.
        let mut merge_enqueue: Option<(usize, usize, String)> = None;
        let mut merge_dequeue_agent_id: Option<usize> = None;
        let mut merge_conflict_info: Option<(usize, String)> = None;

        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            if agent.status == AgentStatus::Starting {
                agent.status = AgentStatus::Running;
            }
            // Phase detection — heuristic, best-effort; only advance, never retreat
            if let Ok(text) = std::str::from_utf8(data) {
                if let Some(detected) = detect_phase(text) {
                    if detected > agent.phase {
                        let prev_phase = agent.phase;
                        agent.phase = detected;
                        debug!(
                            agent_id,
                            task_id = %agent.task.id,
                            from_phase = ?prev_phase,
                            to_phase = ?detected,
                            event = "agent_phase_transition",
                            "agent phase advanced"
                        );
                        // Merge queue: track agents entering/leaving the Merging phase
                        if detected == AgentPhase::Merging {
                            merge_enqueue = Some((agent.id, agent.unit_number, agent.task.id.clone()));
                        } else if prev_phase == AgentPhase::Merging {
                            merge_dequeue_agent_id = Some(agent.id);
                        }
                    }
                }
                // Detect merge conflicts during the Merging phase
                if agent.phase == AgentPhase::Merging {
                    let lower = text.to_lowercase();
                    if lower.contains("conflict")
                        && (lower.contains("merge") || lower.contains("automatic merge failed"))
                        || text.contains("CONFLICT (")
                        || lower.contains("unmerged paths")
                    {
                        merge_conflict_info = Some((agent.unit_number, agent.task.id.clone()));
                    }
                }
            }
        }

        // Process merge queue events (outside borrow scope)
        if let Some((aid, unit, task_id)) = merge_enqueue {
            // Only enqueue if not already in the queue
            if !self.merge_queue.iter().any(|e| e.agent_id == aid) {
                let queue_pos = self.merge_queue.len();
                self.merge_queue.push_back(MergeQueueEntry {
                    agent_id: aid,
                    unit_number: unit,
                    task_id: task_id.clone(),
                    enqueued_at: std::time::Instant::now(),
                });
                if queue_pos == 0 {
                    self.log(
                        LogCategory::System,
                        format!("MERGE-QUEUE: AGENT-{:02} merging {} (queue empty, proceeding)", unit, task_id),
                    );
                } else {
                    self.log(
                        LogCategory::Alert,
                        format!(
                            "MERGE-QUEUE: AGENT-{:02} queued for merge ({}) — {} agent(s) ahead",
                            unit, task_id, queue_pos
                        ),
                    );
                }
            }
        }
        if let Some(aid) = merge_dequeue_agent_id {
            if let Some(pos) = self.merge_queue.iter().position(|e| e.agent_id == aid) {
                let entry = self.merge_queue.remove(pos).unwrap();
                let elapsed = entry.enqueued_at.elapsed().as_secs();
                self.log(
                    LogCategory::System,
                    format!(
                        "MERGE-QUEUE: AGENT-{:02} merge complete for {} ({}s in queue, {} remaining)",
                        entry.unit_number, entry.task_id, elapsed, self.merge_queue.len()
                    ),
                );
            }
        }
        if let Some((unit, task_id)) = merge_conflict_info {
            self.log(
                LogCategory::Alert,
                format!(
                    "MERGE-QUEUE: AGENT-{:02} merge CONFLICT on {} — agent should release lock and resolve",
                    unit, task_id
                ),
            );
        }
        // Throughput tracking: count actual newlines only (no minimum-1 inflation)
        let newlines = data.iter().filter(|&&b| b == b'\n').count() as u16;
        self.lines_this_tick = self.lines_this_tick.saturating_add(newlines);

    }

    /// Write raw bytes to the selected agent's PTY (interactive mode).
    pub fn write_to_agent(&mut self, bytes: &[u8]) -> bool {
        if let Some(agent_id) = self.selected_agent_id {
            if let Some(state) = self.pty_states.get_mut(&agent_id) {
                use std::io::Write;
                return state.writer.write_all(bytes).is_ok();
            }
        }
        false
    }

    /// Resize the PTY for the selected agent.
    pub fn resize_agent_pty(&mut self, agent_id: usize, rows: u16, cols: u16) {
        if let Some(state) = self.pty_states.get_mut(&agent_id) {
            let size = portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };
            let _ = state.master.resize(size);
            // Resize the parser in place to preserve scrollback history.
            // The child process will repaint after receiving SIGWINCH.
            state.parser.screen_mut().set_size(rows, cols);
        }
    }

    /// Count total output lines for an agent.
    ///
    /// Uses two complementary strategies and returns whichever is larger:
    ///  1. **Scrollback method** — `cumulative_scrollback + visible_rows`.
    ///     Accurate on macOS/Linux where the vt100 parser sees real scroll events.
    ///  2. **Newline method** — cumulative count of `\n` bytes in raw PTY data.
    ///     Works on Windows ConPTY where cursor-positioning replaces scrolling,
    ///     so the vt100 parser's scrollback stays at zero.
    ///
    /// Taking the `max` ensures the count keeps incrementing on every platform.
    pub fn agent_line_count(&self, agent_id: usize) -> usize {
        if let Some(state) = self.pty_states.get(&agent_id) {
            let screen = state.parser.screen();
            let (rows, _cols) = screen.size();
            let scrollback_total = state.cumulative_scrollback + rows as usize;
            std::cmp::max(scrollback_total, state.cumulative_newlines)
        } else if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            if agent.total_lines > 0 {
                agent.total_lines
            } else {
                // Legacy fallback for agents without PTY
                agent.output.len()
            }
        } else {
            0
        }
    }

    /// Resize all active PTYs to match the given dimensions, if changed.
    pub fn sync_pty_sizes(&mut self, rows: u16, cols: u16) {
        if rows < 2 || cols < 10 {
            return;
        }
        if self.last_pty_size == (rows, cols) {
            return;
        }
        self.last_pty_size = (rows, cols);
        let ids: Vec<usize> = self.pty_states.keys().copied().collect();
        for id in ids {
            self.resize_agent_pty(id, rows, cols);
        }
    }

    pub fn completion_rate(&self) -> f64 {
        if self.agents.is_empty() {
            return 0.0;
        }
        let completed = self.count_completed() as f64;
        let total = self.agents.len() as f64;
        completed / total * 100.0
    }

    pub fn format_elapsed(secs: u64) -> String {
        let m = secs / 60;
        let s = secs % 60;
        format!("{:02}:{:02}", m, s)
    }

    /// Compute aggregate stats across all loaded history sessions.
    /// Returns (total_sessions, all_time_completed, all_time_failed, avg_duration_secs, total_cost_usd).
    pub fn aggregate_stats(&self) -> (usize, u64, u64, f64, f64) {
        let total_sessions = self.history_sessions.len();
        let all_time_completed: u64 = self.history_sessions
            .iter()
            .map(|s| s.total_completed as u64)
            .sum();
        let all_time_failed: u64 = self.history_sessions
            .iter()
            .map(|s| s.total_failed as u64)
            .sum();

        // Average agent duration across all history sessions
        let (total_elapsed, agent_count): (u64, u64) = self.history_sessions
            .iter()
            .flat_map(|s| s.agents.iter())
            .fold((0u64, 0u64), |(sum, cnt), a| (sum + a.elapsed_secs, cnt + 1));

        let avg_duration = if agent_count > 0 {
            total_elapsed as f64 / agent_count as f64
        } else {
            0.0
        };

        let total_cost: f64 = self.history_sessions
            .iter()
            .map(|s| s.total_cost_usd)
            .sum();

        (total_sessions, all_time_completed, all_time_failed, avg_duration, total_cost)
    }

    // ── Split-pane view methods ──

    /// Auto-populate split pane slots with running agents.
    ///
    /// When 4 or fewer agents are running, panels are fixed (no cycling).
    /// Rotation only kicks in when more than 4 agents are active.
    pub fn auto_fill_split_panes(&mut self) {
        let running: Vec<usize> = self
            .agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .map(|a| a.id)
            .collect();

        // Collect pinned slot→agent mappings first
        let pinned: Vec<(usize, usize)> = (0..4)
            .filter_map(|slot| {
                let id = self.split_pane_agents[slot]?;
                if self.agents.iter().any(|a| a.id == id && a.pinned_to_split == Some(slot)) {
                    Some((slot, id))
                } else {
                    None
                }
            })
            .collect();
        let pinned_ids: Vec<usize> = pinned.iter().map(|&(_, id)| id).collect();

        // Unpinned running agents (candidates for auto-assignment)
        let unpinned_running: Vec<usize> = running
            .iter()
            .filter(|id| !pinned_ids.contains(id))
            .copied()
            .collect();

        // Count available (non-pinned) slots
        let pinned_slots: Vec<usize> = pinned.iter().map(|&(s, _)| s).collect();
        let free_slots: Vec<usize> = (0..4).filter(|s| !pinned_slots.contains(s)).collect();

        if unpinned_running.len() <= free_slots.len() {
            // ≤ 4 agents (accounting for pinned): fixed panels, no rotation
            self.split_pane_rotation_offset = 0;
            let mut unpinned_iter = unpinned_running.iter();
            for &slot in &free_slots {
                // Keep current agent if still running
                if let Some(id) = self.split_pane_agents[slot] {
                    if unpinned_running.contains(&id) {
                        continue;
                    }
                }
                // Fill with next available agent
                loop {
                    match unpinned_iter.next() {
                        Some(&id) => {
                            if self.split_pane_agents.contains(&Some(id)) {
                                continue; // already shown in another slot
                            }
                            self.split_pane_agents[slot] = Some(id);
                            break;
                        }
                        None => {
                            self.split_pane_agents[slot] = None;
                            break;
                        }
                    }
                }
            }
        } else {
            // > 4 agents: rotate through unpinned agents across free slots
            let offset = self.split_pane_rotation_offset % unpinned_running.len();
            for (i, &slot) in free_slots.iter().enumerate() {
                let idx = (offset + i) % unpinned_running.len();
                self.split_pane_agents[slot] = Some(unpinned_running[idx]);
            }
            self.split_pane_rotation_offset =
                (self.split_pane_rotation_offset + free_slots.len()) % unpinned_running.len();
        }
    }

    /// Pin the agent in the focused pane (toggle).
    pub fn toggle_pin_split_pane(&mut self) {
        let slot = self.split_pane_focus;
        if let Some(agent_id) = self.split_pane_agents[slot] {
            let (unit, was_pinned) = {
                let agent = match self.agents.iter_mut().find(|a| a.id == agent_id) {
                    Some(a) => a,
                    None => return,
                };
                let was = agent.pinned_to_split == Some(slot);
                if was {
                    agent.pinned_to_split = None;
                } else {
                    agent.pinned_to_split = Some(slot);
                }
                (agent.unit_number, was)
            };
            if was_pinned {
                self.log(
                    LogCategory::System,
                    format!("Unpinned AGENT-{:02} from pane {}", unit, slot + 1),
                );
            } else {
                self.log(
                    LogCategory::System,
                    format!("Pinned AGENT-{:02} to pane {}", unit, slot + 1),
                );
            }
        }
    }

    /// Number of panes to show based on terminal width.
    pub fn split_pane_count(&self, term_width: u16) -> usize {
        if term_width < 80 {
            1
        } else if term_width < 160 {
            2
        } else {
            4
        }
    }

    /// Scroll up in the focused split pane.
    pub fn split_pane_scroll_up(&mut self) {
        let slot = self.split_pane_focus;
        if let Some(agent_id) = self.split_pane_agents[slot] {
            if let Some(state) = self.pty_states.get(&agent_id) {
                let total = state.parser.screen().size().0 as usize;
                match self.split_pane_scroll[slot] {
                    None => {
                        if total > 0 {
                            self.split_pane_scroll[slot] = Some(total.saturating_sub(1));
                        }
                    }
                    Some(pos) => {
                        if pos > 0 {
                            self.split_pane_scroll[slot] = Some(pos - 1);
                        }
                    }
                }
            }
        }
    }

    /// Scroll down in the focused split pane.
    pub fn split_pane_scroll_down(&mut self) {
        let slot = self.split_pane_focus;
        if let Some(agent_id) = self.split_pane_agents[slot] {
            if let Some(state) = self.pty_states.get(&agent_id) {
                let total = state.parser.screen().size().0 as usize;
                if let Some(pos) = self.split_pane_scroll[slot] {
                    if pos + 1 >= total {
                        self.split_pane_scroll[slot] = None;
                    } else {
                        self.split_pane_scroll[slot] = Some(pos + 1);
                    }
                }
            }
        }
    }

    /// Enter AgentDetail for the agent in the focused split pane.
    pub fn split_pane_enter_detail(&mut self) {
        let slot = self.split_pane_focus;
        if let Some(agent_id) = self.split_pane_agents[slot] {
            self.selected_agent_id = Some(agent_id);
            self.agent_output_scroll = None;
            self.active_view = View::AgentDetail;
        }
    }

    /// Persist raw PTY log bytes to disk for the given agent.
    /// On the first call, creates the file with a metadata header.
    /// On subsequent calls, appends only the new bytes since the last flush.
    /// Called automatically on agent completion/failure and periodically during long runs.
    pub fn persist_agent_pty_log(&mut self, agent_id: usize) {
        let agent = match self.agents.iter().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return,
        };

        let new_bytes = agent.raw_pty_log.len();
        let flushed = agent.pty_log_flushed_bytes;
        if new_bytes <= flushed {
            return; // nothing new to write
        }

        let logs_dir = PathBuf::from(".obelisk").join("logs");
        if let Err(e) = std::fs::create_dir_all(&logs_dir) {
            tracing::warn!("Failed to create PTY log dir: {}", e);
            return;
        }

        let filename = format!("agent-{:02}-{}.log", agent.unit_number, agent.task.id);
        let path = logs_dir.join(&filename);

        use std::io::Write;

        if flushed == 0 {
            // First flush — write header then all bytes so far
            let mut file = match std::fs::File::create(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to create PTY log file {}: {}", path.display(), e);
                    return;
                }
            };
            let header = format!(
                "=== Obelisk PTY Log ===\nAgent: AGENT-{:02}\nTask: {} ({})\nRuntime: {}\nModel: {}\nStarted: {}\n\n",
                agent.unit_number,
                agent.task.id,
                agent.task.title,
                agent.runtime.name(),
                agent.model,
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            );
            let _ = file.write_all(header.as_bytes());
            let _ = file.write_all(&agent.raw_pty_log[..new_bytes]);
        } else {
            // Append only new bytes
            let mut file = match std::fs::OpenOptions::new().append(true).open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to append to PTY log file {}: {}", path.display(), e);
                    return;
                }
            };
            let _ = file.write_all(&agent.raw_pty_log[flushed..new_bytes]);
        }

        // Update flushed byte count (need mutable borrow)
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.pty_log_flushed_bytes = new_bytes;
        }
    }

    /// Export the selected agent's log to a file.
    /// Writes both raw PTY output and parsed screen content.
    /// Returns the path on success or an error message on failure.
    pub fn export_agent_log(&mut self) -> Result<String, String> {
        let agent_id = self.selected_agent_id.ok_or("No agent selected")?;
        let agent = self.agents.iter().find(|a| a.id == agent_id)
            .ok_or("Agent not found")?;

        // Build export path: logs/<task_id>-<timestamp>.log
        let logs_dir = PathBuf::from("logs");
        if !logs_dir.exists() {
            std::fs::create_dir_all(&logs_dir)
                .map_err(|e| format!("Failed to create logs dir: {}", e))?;
        }
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let filename = format!("{}-{}.log", agent.task.id, timestamp);
        let path = logs_dir.join(&filename);

        let mut content = String::new();

        // ── Header ──
        content.push_str("=== Obelisk Agent Log Export ===\n");
        content.push_str(&format!("Agent:     AGENT-{:02}\n", agent.unit_number));
        content.push_str(&format!("Task:      {} ({})\n", agent.task.id, agent.task.title));
        content.push_str(&format!("Runtime:   {}\n", agent.runtime.name()));
        content.push_str(&format!("Model:     {}\n", agent.model));
        content.push_str(&format!("Status:    {:?}\n", agent.status));
        content.push_str(&format!("Phase:     {}\n", agent.phase.label()));
        content.push_str(&format!("Elapsed:   {}s\n", agent.elapsed_secs));
        if let Some(code) = agent.exit_code {
            content.push_str(&format!("Exit code: {}\n", code));
        }
        content.push_str(&format!("Exported:  {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        content.push('\n');

        // ── Section 1: Parsed screen content ──
        content.push_str("════════════════════════════════════════════════════════════════════════════════\n");
        content.push_str("PARSED SCREEN CONTENT\n");
        content.push_str("════════════════════════════════════════════════════════════════════════════════\n\n");

        if let Some(pty_state) = self.pty_states.get(&agent_id) {
            let screen = pty_state.parser.screen();
            let (rows, cols) = screen.size();
            let scrollback = screen.scrollback();

            content.push_str(&format!("Terminal size: {}x{}, scrollback: {} lines\n\n", cols, rows, scrollback));

            // Extract visible screen content row by row
            for row in 0..rows {
                let line: String = (0..cols)
                    .map(|col| {
                        screen.cell(row, col)
                            .map(|c| {
                                let s = c.contents();
                                if s.is_empty() { ' ' } else { s.chars().next().unwrap_or(' ') }
                            })
                            .unwrap_or(' ')
                    })
                    .collect();
                content.push_str(line.trim_end());
                content.push('\n');
            }
        } else {
            // Fallback: legacy output buffer
            content.push_str("(No PTY state — using legacy output buffer)\n\n");
            for line in &agent.output {
                content.push_str(line);
                content.push('\n');
            }
        }

        // ── Section 2: Raw PTY output ──
        content.push_str("\n════════════════════════════════════════════════════════════════════════════════\n");
        content.push_str("RAW PTY OUTPUT\n");
        content.push_str("════════════════════════════════════════════════════════════════════════════════\n\n");

        if agent.raw_pty_log.is_empty() {
            content.push_str("(No raw PTY data captured)\n");
        } else {
            content.push_str(&format!("Raw bytes: {} total\n\n", agent.raw_pty_log.len()));
            // Write raw bytes as lossy UTF-8 (preserves ANSI escape sequences)
            content.push_str(&String::from_utf8_lossy(&agent.raw_pty_log));
        }

        // Write to file
        std::fs::write(&path, &content)
            .map_err(|e| format!("Failed to write log: {}", e))?;

        let path_str = path.to_string_lossy().to_string();
        self.log(
            LogCategory::System,
            format!("AGENT-{:02} log exported to {}", agent.unit_number, path_str),
        );
        Ok(path_str)
    }
}

/// Return the path to the sessions JSONL file. Tries `.beads/obelisk_sessions.jsonl`
/// relative to the current working directory (where the binary is run from).
fn sessions_file_path() -> std::path::PathBuf {
    std::path::PathBuf::from(".beads").join("obelisk_sessions.jsonl")
}

/// Load all session records from the persistent JSONL file.
/// Returns an empty Vec if the file doesn't exist or can't be parsed.
pub fn load_history_sessions() -> Vec<SessionRecord> {
    let path = sessions_file_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<SessionRecord>(line).ok())
        .collect()
}

/// Generate a simple session ID using the current timestamp (no external UUID crate needed).
fn generate_session_id() -> String {
    let now = chrono::Local::now();
    format!("sess-{}", now.format("%Y%m%d-%H%M%S"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn test_task() -> BeadTask {
        BeadTask {
            id: "test-001".into(),
            title: "test task".into(),
            status: "open".into(),
            priority: None,
            issue_type: None,
            assignee: None,
            labels: None,
            description: None,
            created_at: None,
        }
    }

    fn test_agent(id: usize, status: AgentStatus) -> AgentInstance {
        AgentInstance {
            id,
            unit_number: 1,
            task: test_task(),
            runtime: Runtime::ClaudeCode,
            model: "test-model".into(),
            status,
            phase: AgentPhase::Detecting,
            output: VecDeque::new(),
            started_at: std::time::Instant::now(),
            elapsed_secs: 0,
            exit_code: None,
            pid: None,
            retry_count: 0,
            worktree_path: None,
            worktree_cleaned: false,
            template_name: String::new(),
            pinned_to_split: None,
            total_lines: 0,
            raw_pty_log: Vec::new(),
            pty_log_flushed_bytes: 0,
            started_at_utc: chrono::Utc::now(),
            usage: None,
        }
    }

    #[test]
    fn exit_code_zero_marks_completed() {
        let mut app = App::new();
        app.agents.push(test_agent(42, AgentStatus::Running));

        app.on_agent_exited(42, Some(0));

        let agent = app.agents.iter().find(|a| a.id == 42).unwrap();
        assert_eq!(agent.status, AgentStatus::Completed);
        assert_eq!(agent.exit_code, Some(0));
    }

    #[test]
    fn exit_code_nonzero_marks_failed() {
        let mut app = App::new();
        app.agents.push(test_agent(43, AgentStatus::Running));

        app.on_agent_exited(43, Some(1));

        let agent = app.agents.iter().find(|a| a.id == 43).unwrap();
        assert_eq!(agent.status, AgentStatus::Failed);
    }

    #[test]
    fn exit_does_not_overwrite_completed_status() {
        let mut app = App::new();
        app.agents.push(test_agent(44, AgentStatus::Completed));

        // Simulate: force_complete set Completed, then exit watcher fires
        // with non-zero (process was killed after being marked complete)
        app.on_agent_exited(44, Some(1));

        let agent = app.agents.iter().find(|a| a.id == 44).unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Completed,
            "exit watcher must not overwrite Completed status"
        );
    }

    #[test]
    fn exit_does_not_overwrite_failed_status() {
        let mut app = App::new();
        app.agents.push(test_agent(45, AgentStatus::Failed));

        // Simulate: kill_selected_agent set Failed, then exit watcher fires
        app.on_agent_exited(45, Some(0));

        let agent = app.agents.iter().find(|a| a.id == 45).unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Failed,
            "exit watcher must not overwrite Failed status"
        );
    }

    #[test]
    fn detect_phase_ordering() {
        assert_eq!(detect_phase("bd close obelisk-1"), Some(AgentPhase::Closing));
        assert_eq!(detect_phase("git merge --no-ff"), Some(AgentPhase::Merging));
        assert_eq!(detect_phase("cargo test"), Some(AgentPhase::Verifying));
        assert_eq!(detect_phase("bd update x --notes 'done'"), Some(AgentPhase::Implementing));
        assert_eq!(detect_phase("git worktree add ../wt"), Some(AgentPhase::Worktree));
        assert_eq!(detect_phase("bd update x --claim"), Some(AgentPhase::Claiming));
        assert_eq!(detect_phase("just some text"), None);
    }

    #[test]
    fn detect_phase_highest_wins() {
        // Text containing both claim and close markers — close is checked first
        let text = "--claim and bd close";
        assert_eq!(detect_phase(text), Some(AgentPhase::Closing));
    }

    #[test]
    fn force_complete_sets_completed() {
        let mut app = App::new();
        app.agents.push(test_agent(50, AgentStatus::Running));

        let result = app.force_complete_agent(50);
        assert!(result.is_some());

        let agent = app.agents.iter().find(|a| a.id == 50).unwrap();
        assert_eq!(agent.status, AgentStatus::Completed);
        assert_eq!(app.total_completed, 1);
        assert_eq!(app.total_failed, 0);
    }

    #[test]
    fn force_complete_ignores_finished_agents() {
        let mut app = App::new();
        app.agents.push(test_agent(51, AgentStatus::Failed));

        let result = app.force_complete_agent(51);
        assert!(result.is_none(), "should not force-complete a finished agent");

        let agent = app.agents.iter().find(|a| a.id == 51).unwrap();
        assert_eq!(agent.status, AgentStatus::Failed);
    }

    #[test]
    fn force_complete_then_exit_preserves_completed() {
        let mut app = App::new();
        app.agents.push(test_agent(52, AgentStatus::Running));

        // Mark complete manually
        app.force_complete_agent(52);
        // Then exit watcher fires with non-zero (SIGTERM)
        app.on_agent_exited(52, Some(143));

        let agent = app.agents.iter().find(|a| a.id == 52).unwrap();
        assert_eq!(
            agent.status,
            AgentStatus::Completed,
            "exit watcher must not overwrite force-completed status"
        );
    }

    // ── Phase detection ──────────────────────────────────────────

    #[test]
    fn detect_phase_claiming() {
        assert_eq!(detect_phase("bd update x --claim --json"), Some(AgentPhase::Claiming));
    }

    #[test]
    fn detect_phase_worktree() {
        assert_eq!(detect_phase("git worktree add ../worktree-abc"), Some(AgentPhase::Worktree));
    }

    #[test]
    fn detect_phase_implementing() {
        assert_eq!(detect_phase("bd update x --notes \"doing stuff\""), Some(AgentPhase::Implementing));
    }

    #[test]
    fn detect_phase_verifying_cargo_test() {
        assert_eq!(detect_phase("cargo test --release"), Some(AgentPhase::Verifying));
    }

    #[test]
    fn detect_phase_verifying_cargo_clippy() {
        assert_eq!(detect_phase("cargo clippy -- -D warnings"), Some(AgentPhase::Verifying));
    }

    #[test]
    fn detect_phase_verifying_cargo_check() {
        assert_eq!(detect_phase("cargo check"), Some(AgentPhase::Verifying));
    }

    #[test]
    fn detect_phase_merging() {
        assert_eq!(detect_phase("git merge abc --no-ff -m \"msg\""), Some(AgentPhase::Merging));
    }

    #[test]
    fn detect_phase_closing() {
        assert_eq!(detect_phase("bd close abc --reason done"), Some(AgentPhase::Closing));
    }

    #[test]
    fn detect_phase_none_for_unrelated_text() {
        assert_eq!(detect_phase("echo hello world"), None);
    }

    #[test]
    fn detect_phase_highest_wins_when_multiple() {
        // "bd close" and "--claim" both present: closing > claiming
        assert_eq!(detect_phase("bd close --claim"), Some(AgentPhase::Closing));
    }

    // ── Issue-closed completion via polling (obelisk-ip4) ──────

    #[test]
    fn on_issue_closed_marks_running_agent_completed() {
        let mut app = App::new();
        let agent = test_agent(50, AgentStatus::Running);
        app.agents.push(agent);

        app.on_issue_closed(50);

        let agent = app.agents.iter().find(|a| a.id == 50).unwrap();
        assert_eq!(agent.phase, AgentPhase::Done);
        assert_eq!(agent.status, AgentStatus::Completed);
        assert_eq!(app.total_completed, 1);
    }

    #[test]
    fn on_issue_closed_ignores_already_completed_agent() {
        let mut app = App::new();
        let mut agent = test_agent(51, AgentStatus::Completed);
        agent.phase = AgentPhase::Done;
        app.agents.push(agent);

        app.on_issue_closed(51);

        // Should not increment completion count again
        assert_eq!(app.total_completed, 0);
    }

    #[test]
    fn on_issue_closed_ignores_unknown_agent() {
        let mut app = App::new();
        // No agents registered — should not panic
        app.on_issue_closed(999);
        assert_eq!(app.total_completed, 0);
    }

    #[test]
    fn exit_after_issue_closed_does_not_double_count() {
        let mut app = App::new();
        let agent = test_agent(53, AgentStatus::Running);
        app.agents.push(agent);

        // First: issue closure via polling marks Completed
        app.on_issue_closed(53);
        assert_eq!(app.total_completed, 1);

        // Then: process exits naturally — should not increment again
        app.on_agent_exited(53, Some(0));
        assert_eq!(app.total_completed, 1, "must not double-count completion");

        let agent = app.agents.iter().find(|a| a.id == 53).unwrap();
        assert_eq!(agent.status, AgentStatus::Completed);
        assert_eq!(agent.exit_code, Some(0));
    }

    // ── Error pattern detection ──────────────────────────────────

    #[test]
    fn detect_errors_compilation() {
        let lines = vec!["error[E0308]: mismatched types", "  --> src/main.rs:10:5"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.iter().any(|e| e.contains("Compilation")));
    }

    #[test]
    fn detect_errors_test_failure() {
        let lines = vec!["test result: FAILED. 2 passed; 1 failed; 0 ignored"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.iter().any(|e| e.contains("Test failure")));
    }

    #[test]
    fn detect_errors_panic() {
        let lines = vec!["thread 'main' panicked at 'index out of bounds'"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.iter().any(|e| e.contains("Panic")));
    }

    #[test]
    fn detect_errors_permission_denied() {
        let lines = vec!["Error: Permission denied (os error 13)"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.iter().any(|e| e.contains("Permission denied")));
    }

    #[test]
    fn detect_errors_merge_conflict() {
        let lines = vec!["CONFLICT: merge conflict in src/main.rs"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.iter().any(|e| e.contains("merge conflict")));
    }

    #[test]
    fn detect_errors_empty_for_clean_output() {
        let lines = vec!["running 5 tests", "test result: ok. 5 passed; 0 ignored"];
        let errors = detect_error_patterns(&lines);
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn format_elapsed_various() {
        assert_eq!(App::format_elapsed(0), "00:00");
        assert_eq!(App::format_elapsed(59), "00:59");
        assert_eq!(App::format_elapsed(60), "01:00");
        assert_eq!(App::format_elapsed(3661), "61:01");
    }

    // ── Agent lifecycle ──────────────────────────────────────────

    #[test]
    fn on_agent_output_transitions_starting_to_running() {
        let mut app = App::new();
        app.agents.push(test_agent(10, AgentStatus::Starting));

        app.on_agent_output(10, "first line".into());

        let agent = app.agents.iter().find(|a| a.id == 10).unwrap();
        assert_eq!(agent.status, AgentStatus::Running);
        assert_eq!(agent.output.len(), 1);
    }

    #[test]
    fn on_agent_output_caps_at_10000_lines() {
        let mut app = App::new();
        app.agents.push(test_agent(11, AgentStatus::Running));

        for i in 0..10005 {
            app.on_agent_output(11, format!("line {}", i));
        }

        let agent = app.agents.iter().find(|a| a.id == 11).unwrap();
        assert_eq!(agent.output.len(), 10000);
        // Oldest lines should have been discarded
        assert!(agent.output.front().unwrap().contains("line 5"));
    }

    #[test]
    fn active_agent_count_only_counts_starting_and_running() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Starting));
        app.agents.push(test_agent(2, AgentStatus::Running));
        app.agents.push(test_agent(3, AgentStatus::Completed));
        app.agents.push(test_agent(4, AgentStatus::Failed));

        assert_eq!(app.active_agent_count(), 2);
    }

    #[test]
    fn on_agent_exited_increments_counters() {
        let mut app = App::new();
        app.agents.push(test_agent(50, AgentStatus::Running));
        app.agents.push(test_agent(51, AgentStatus::Running));

        app.on_agent_exited(50, Some(0));
        assert_eq!(app.total_completed, 1);
        assert_eq!(app.total_failed, 0);

        app.on_agent_exited(51, Some(1));
        assert_eq!(app.total_completed, 1);
        assert_eq!(app.total_failed, 1);
    }

    #[test]
    fn on_agent_exited_detaches_interactive_mode() {
        let mut app = App::new();
        app.agents.push(test_agent(60, AgentStatus::Running));
        app.interactive_mode = true;
        app.selected_agent_id = Some(60);

        app.on_agent_exited(60, Some(0));

        assert!(!app.interactive_mode, "interactive mode should be detached on exit");
    }

    #[test]
    fn dismiss_finished_agents() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Running));
        app.agents.push(test_agent(2, AgentStatus::Completed));
        app.agents.push(test_agent(3, AgentStatus::Failed));

        let dismissed = app.dismiss_all_finished();
        assert_eq!(dismissed, 2);
        assert_eq!(app.agents.len(), 1);
        assert_eq!(app.agents[0].id, 1);
    }

    #[test]
    fn dismiss_all_finished_clears_selected_if_dismissed() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Completed));
        app.selected_agent_id = Some(1);

        app.dismiss_all_finished();

        assert!(app.selected_agent_id.is_none());
    }

    // ── Poll result handling ─────────────────────────────────────

    #[test]
    fn on_poll_result_filters_claimed_tasks() {
        let mut app = App::new();
        app.claimed_task_ids.insert("t-1".into());

        let tasks = vec![
            BeadTask { id: "t-1".into(), title: "Claimed".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
            BeadTask { id: "t-2".into(), title: "New".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];

        app.on_poll_result(tasks);

        assert_eq!(app.ready_tasks.len(), 1);
        assert_eq!(app.ready_tasks[0].id, "t-2");
    }

    #[test]
    fn on_poll_result_resets_failure_state() {
        let mut app = App::new();
        app.last_poll_ok = false;
        app.consecutive_poll_failures = 5;
        app.last_poll_error = Some("timeout".into());

        app.on_poll_result(vec![]);

        assert!(app.last_poll_ok);
        assert_eq!(app.consecutive_poll_failures, 0);
        assert!(app.last_poll_error.is_none());
    }

    #[test]
    fn on_poll_result_filters_out_epics() {
        let mut app = App::new();

        let tasks = vec![
            BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                       priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "t-1".into(), title: "Task".into(), status: "open".into(),
                       priority: Some(2), issue_type: Some("task".into()), assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "b-1".into(), title: "Bug".into(), status: "open".into(),
                       priority: Some(0), issue_type: Some("bug".into()), assignee: None,
                       labels: None, description: None, created_at: None },
        ];

        app.on_poll_result(tasks);

        assert_eq!(app.ready_tasks.len(), 2, "epic should be filtered out");
        assert!(app.ready_tasks.iter().all(|t| !t.is_epic()),
                "no epics should be in the ready queue");
        assert!(app.ready_tasks.iter().any(|t| t.id == "t-1"));
        assert!(app.ready_tasks.iter().any(|t| t.id == "b-1"));
    }

    #[test]
    fn on_poll_result_filters_epics_with_none_issue_type_kept() {
        let mut app = App::new();

        let tasks = vec![
            BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                       priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "t-1".into(), title: "Default type".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];

        app.on_poll_result(tasks);

        assert_eq!(app.ready_tasks.len(), 1);
        assert_eq!(app.ready_tasks[0].id, "t-1",
                   "tasks with no issue_type (defaults to task) should be kept");
    }

    #[test]
    fn get_spawn_info_blocks_epic() {
        let mut app = App::new();
        app.ready_tasks = vec![
            BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                       priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.task_list_state.select(Some(0));

        let result = app.get_spawn_info();
        assert!(result.is_none(), "should refuse to spawn an agent for an epic");
    }

    #[test]
    fn on_poll_failed_increments_failure_count() {
        let mut app = App::new();

        app.on_poll_failed("connection refused".into());
        assert_eq!(app.consecutive_poll_failures, 1);
        assert!(!app.last_poll_ok);

        app.on_poll_failed("timeout".into());
        assert_eq!(app.consecutive_poll_failures, 2);
    }

    // ── Sorting ──────────────────────────────────────────────────

    #[test]
    fn sort_ready_tasks_by_priority() {
        let mut app = App::new();
        app.sort_mode = SortMode::Priority;
        app.ready_tasks = vec![
            BeadTask { id: "c".into(), title: "Low".into(), status: "open".into(),
                       priority: Some(3), issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
            BeadTask { id: "a".into(), title: "High".into(), status: "open".into(),
                       priority: Some(0), issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
            BeadTask { id: "b".into(), title: "Med".into(), status: "open".into(),
                       priority: Some(1), issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];

        app.sort_ready_tasks();

        assert_eq!(app.ready_tasks[0].id, "a"); // P0
        assert_eq!(app.ready_tasks[1].id, "b"); // P1
        assert_eq!(app.ready_tasks[2].id, "c"); // P3
    }

    #[test]
    fn sort_ready_tasks_by_name() {
        let mut app = App::new();
        app.sort_mode = SortMode::Name;
        app.ready_tasks = vec![
            BeadTask { id: "1".into(), title: "Zebra".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
            BeadTask { id: "2".into(), title: "Apple".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];

        app.sort_ready_tasks();

        assert_eq!(app.ready_tasks[0].title, "Apple");
        assert_eq!(app.ready_tasks[1].title, "Zebra");
    }

    #[test]
    fn sort_ready_tasks_by_type() {
        let mut app = App::new();
        app.sort_mode = SortMode::Type;
        app.ready_tasks = vec![
            BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                       priority: Some(1), issue_type: Some("task".into()), assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "2".into(), title: "B".into(), status: "open".into(),
                       priority: Some(1), issue_type: Some("bug".into()), assignee: None,
                       labels: None, description: None, created_at: None },
        ];

        app.sort_ready_tasks();

        assert_eq!(app.ready_tasks[0].issue_type.as_deref(), Some("bug"));
        assert_eq!(app.ready_tasks[1].issue_type.as_deref(), Some("task"));
    }

    // ── Filtering ────────────────────────────────────────────────

    #[test]
    fn filtered_tasks_with_type_filter() {
        let mut app = App::new();
        app.ready_tasks = vec![
            BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                       priority: None, issue_type: Some("bug".into()), assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "2".into(), title: "T".into(), status: "open".into(),
                       priority: None, issue_type: Some("task".into()), assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.type_filter.insert("bug".into());

        let filtered = app.filtered_tasks();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].issue_type.as_deref(), Some("bug"));
    }

    #[test]
    fn filtered_tasks_with_priority_filter() {
        let mut app = App::new();
        app.ready_tasks = vec![
            BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                       priority: Some(0), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "2".into(), title: "T".into(), status: "open".into(),
                       priority: Some(3), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.priority_filter = Some(0..=1);

        let filtered = app.filtered_tasks();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].priority, Some(0));
    }

    #[test]
    fn filtered_agents_respects_status_filter() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Running));
        app.agents.push(test_agent(2, AgentStatus::Completed));
        app.agents.push(test_agent(3, AgentStatus::Failed));

        app.agent_status_filter = AgentStatusFilter::Failed;
        let filtered = app.filtered_agents();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, 3);
    }

    // ── Navigation ───────────────────────────────────────────────

    #[test]
    fn toggle_focus_switches_between_panels() {
        let mut app = App::new();
        assert_eq!(app.focus, Focus::ReadyQueue);

        app.toggle_focus();
        assert_eq!(app.focus, Focus::BlockedQueue);

        app.toggle_focus();
        assert_eq!(app.focus, Focus::AgentList);

        app.toggle_focus();
        assert_eq!(app.focus, Focus::ReadyQueue);
    }

    #[test]
    fn navigate_wraps_around_in_ready_queue() {
        let mut app = App::new();
        app.active_view = View::Dashboard;
        app.focus = Focus::ReadyQueue;
        app.ready_tasks = vec![
            BeadTask { id: "a".into(), title: "A".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
            BeadTask { id: "b".into(), title: "B".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];
        app.task_list_state.select(Some(0));

        // Navigate up from 0 → wraps to last
        app.navigate_up();
        assert_eq!(app.task_list_state.selected(), Some(1));

        // Navigate down from last → wraps to 0
        app.navigate_down();
        assert_eq!(app.task_list_state.selected(), Some(0));
    }

    // ── On tick ──────────────────────────────────────────────────

    #[test]
    fn on_tick_increments_frame_count() {
        let mut app = App::new();
        let before = app.frame_count;
        app.on_tick();
        assert_eq!(app.frame_count, before + 1);
    }

    #[test]
    fn on_tick_decrements_poll_countdown() {
        let mut app = App::new();
        app.poll_countdown = 5.0;
        app.on_tick();
        assert!(app.poll_countdown < 5.0);
    }

    #[test]
    fn on_tick_clears_expired_alert() {
        let mut app = App::new();
        app.alert_message = Some(("Test alert".into(), 0)); // Already expired
        app.frame_count = 1;
        app.on_tick(); // frame_count becomes 2, which is > 0
        assert!(app.alert_message.is_none());
    }

    // ── Model selection ──────────────────────────────────────────

    #[test]
    fn selected_model_returns_valid_model() {
        let app = App::new();
        let model = app.selected_model();
        assert!(!model.is_empty());
    }

    #[test]
    fn cycle_model_advances_index() {
        let mut app = App::new();
        let first = app.selected_model().to_string();
        app.cycle_model();
        let second = app.selected_model().to_string();
        assert_ne!(first, second, "model should change after cycling");
    }

    #[test]
    fn cycle_model_wraps_around() {
        let mut app = App::new();
        let num_models = app.selected_runtime.models().len();
        let first = app.selected_model().to_string();
        for _ in 0..num_models {
            app.cycle_model();
        }
        assert_eq!(app.selected_model(), first, "should wrap back to first model");
    }

    #[test]
    fn completion_rate_with_no_agents() {
        let app = App::new();
        assert_eq!(app.completion_rate(), 0.0);
    }

    #[test]
    fn completion_rate_calculates_correctly() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Completed));
        app.agents.push(test_agent(2, AgentStatus::Failed));

        let rate = app.completion_rate();
        assert!((rate - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completion_rate_unaffected_by_dismiss() {
        let mut app = App::new();
        // 4 agents: 2 completed, 1 failed, 1 running
        app.agents.push(test_agent(1, AgentStatus::Completed));
        app.agents.push(test_agent(2, AgentStatus::Completed));
        app.agents.push(test_agent(3, AgentStatus::Failed));
        app.agents.push(test_agent(4, AgentStatus::Running));
        assert!((app.completion_rate() - 50.0).abs() < f64::EPSILON);

        // Dismiss the 2 completed + 1 failed agents
        app.agents.retain(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running));
        // Only running agent remains → 0% completed, not inflated
        assert!((app.completion_rate() - 0.0).abs() < f64::EPSILON);
    }

    // ── Worktree management ──────────────────────────────────────

    #[test]
    fn on_worktree_orphans_sets_alert() {
        let mut app = App::new();
        app.on_worktree_orphans(vec!["/tmp/worktree-abc".into(), "/tmp/worktree-def".into()]);

        assert!(app.alert_message.is_some());
        let (msg, _) = app.alert_message.as_ref().unwrap();
        assert!(msg.contains("2"), "alert should mention count");
    }

    #[test]
    fn on_worktree_cleaned_marks_agent() {
        let mut app = App::new();
        let mut agent = test_agent(1, AgentStatus::Completed);
        agent.worktree_path = Some("/tmp/worktree-test".into());
        app.agents.push(agent);

        app.on_worktree_cleaned(vec!["/tmp/worktree-test".into()], vec![]);

        let agent = app.agents.iter().find(|a| a.id == 1).unwrap();
        assert!(agent.worktree_cleaned);
    }

    #[test]
    fn selected_agent_worktree_returns_none_when_cleaned() {
        let mut app = App::new();
        let mut agent = test_agent(1, AgentStatus::Completed);
        agent.worktree_path = Some("/tmp/worktree-test".into());
        agent.worktree_cleaned = true;
        app.agents.push(agent);
        app.selected_agent_id = Some(1);

        assert!(app.selected_agent_worktree().is_none());
    }

    #[test]
    fn selected_agent_worktree_returns_path_when_not_cleaned() {
        let mut app = App::new();
        let mut agent = test_agent(1, AgentStatus::Running);
        agent.worktree_path = Some("/tmp/worktree-test".into());
        app.agents.push(agent);
        app.selected_agent_id = Some(1);

        assert_eq!(app.selected_agent_worktree(), Some("/tmp/worktree-test".into()));
    }

    // ── Worktree scan processing ─────────────────────────────────

    #[test]
    fn on_worktree_scanned_classifies_active_worktrees() {
        let mut app = App::new();
        let mut agent = test_agent(1, AgentStatus::Running);
        agent.task = BeadTask {
            id: "abc".into(),
            title: "task".into(),
            status: "in_progress".into(),
            priority: None, issue_type: None, assignee: None, labels: None,
            description: None, created_at: None,
        };
        agent.worktree_path = Some("/tmp/worktree-abc".into());
        app.agents.push(agent);

        app.on_worktree_scanned(vec![("/tmp/worktree-abc".into(), "abc".into())]);

        assert_eq!(app.worktree_entries.len(), 1);
        assert_eq!(app.worktree_entries[0].status, WorktreeStatus::Active);
    }

    #[test]
    fn on_worktree_scanned_classifies_orphaned_worktrees() {
        let mut app = App::new();
        // No agents registered

        app.on_worktree_scanned(vec![("/tmp/worktree-xyz".into(), "xyz".into())]);

        assert_eq!(app.worktree_entries.len(), 1);
        assert_eq!(app.worktree_entries[0].status, WorktreeStatus::Orphaned);
    }

    // ── Jump-to-issue ────────────────────────────────────────────

    #[test]
    fn jump_matches_searches_ready_queue_and_agents() {
        let mut app = App::new();
        app.ready_tasks = vec![
            BeadTask { id: "obelisk-abc".into(), title: "T".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];
        let mut agent = test_agent(1, AgentStatus::Running);
        agent.task.id = "obelisk-def".into();
        app.agents.push(agent);

        app.jump_query = "obelisk".into();
        let matches = app.jump_matches();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn jump_matches_is_case_insensitive() {
        let mut app = App::new();
        app.ready_tasks = vec![
            BeadTask { id: "ABC-123".into(), title: "T".into(), status: "open".into(),
                       priority: None, issue_type: None, assignee: None, labels: None,
                       description: None, created_at: None },
        ];

        app.jump_query = "abc".into();
        let matches = app.jump_matches();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn jump_execute_returns_false_for_empty_query() {
        let mut app = App::new();
        app.jump_query = "".into();
        assert!(!app.jump_execute());
    }

    // ── Split-pane ───────────────────────────────────────────────

    #[test]
    fn split_pane_count_scales_with_width() {
        let app = App::new();
        assert_eq!(app.split_pane_count(60), 1);
        assert_eq!(app.split_pane_count(100), 2);
        assert_eq!(app.split_pane_count(200), 4);
    }

    #[test]
    fn auto_fill_fixed_panels_when_4_or_fewer_agents() {
        let mut app = App::new();
        // Add 3 running agents
        app.agents.push(test_agent(1, AgentStatus::Running));
        app.agents.push(test_agent(2, AgentStatus::Running));
        app.agents.push(test_agent(3, AgentStatus::Running));

        app.auto_fill_split_panes();
        let first_fill = app.split_pane_agents;

        // Call again — panels should NOT rotate
        app.auto_fill_split_panes();
        assert_eq!(app.split_pane_agents, first_fill);

        // Call many more times — still stable
        for _ in 0..20 {
            app.auto_fill_split_panes();
        }
        assert_eq!(app.split_pane_agents, first_fill);

        // All 3 agents should be visible, 4th slot empty
        assert!(app.split_pane_agents[..3].iter().all(|s| s.is_some()));
        assert_eq!(app.split_pane_agents[3], None);
    }

    #[test]
    fn auto_fill_rotates_when_more_than_4_agents() {
        let mut app = App::new();
        // Add 6 running agents
        for i in 1..=6 {
            app.agents.push(test_agent(i, AgentStatus::Running));
        }

        app.auto_fill_split_panes();
        let first_fill = app.split_pane_agents;
        // All 4 slots should be occupied
        assert!(first_fill.iter().all(|s| s.is_some()));

        // Call again — panels should rotate (different agents shown)
        app.auto_fill_split_panes();
        assert_ne!(app.split_pane_agents, first_fill);
    }

    #[test]
    fn auto_fill_respects_pinned_agents_during_rotation() {
        let mut app = App::new();
        for i in 1..=6 {
            app.agents.push(test_agent(i, AgentStatus::Running));
        }

        // Pin agent 1 to slot 0
        app.split_pane_agents[0] = Some(1);
        app.agents[0].pinned_to_split = Some(0);

        app.auto_fill_split_panes();
        assert_eq!(app.split_pane_agents[0], Some(1)); // pinned stays

        // Rotate multiple times — pinned agent stays in slot 0
        for _ in 0..10 {
            app.auto_fill_split_panes();
            assert_eq!(app.split_pane_agents[0], Some(1));
        }
    }

    #[test]
    fn auto_fill_replaces_finished_agents_in_fixed_mode() {
        let mut app = App::new();
        app.agents.push(test_agent(1, AgentStatus::Running));
        app.agents.push(test_agent(2, AgentStatus::Running));
        app.agents.push(test_agent(3, AgentStatus::Running));

        app.auto_fill_split_panes();
        assert!(app.split_pane_agents.contains(&Some(1)));

        // Agent 1 finishes, agent 4 starts
        app.agents[0].status = AgentStatus::Completed;
        app.agents.push(test_agent(4, AgentStatus::Running));

        app.auto_fill_split_panes();
        // Agent 1 should be gone, agent 4 should appear
        assert!(!app.split_pane_agents.contains(&Some(1)));
        assert!(app.split_pane_agents.contains(&Some(4)));
    }

    // ── Log ──────────────────────────────────────────────────────

    #[test]
    fn log_entries_are_capped_at_500() {
        let mut app = App::new();
        for i in 0..510 {
            app.log(LogCategory::System, format!("msg {}", i));
        }
        assert!(app.event_log.len() <= 500);
    }

    #[test]
    fn log_entry_has_timestamp() {
        let mut app = App::new();
        app.log(LogCategory::System, "test".into());
        assert!(!app.event_log[0].timestamp.is_empty());
    }

    // ── Config parsing ───────────────────────────────────────────

    #[test]
    fn config_parse_unknown_runtime_defaults_to_claude() {
        let toml_str = r#"
            [orchestrator]
            runtime = "unknown_runtime"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let runtime = match config.orchestrator.unwrap().runtime.unwrap().as_str() {
            "claude" => Runtime::ClaudeCode,
            "codex" => Runtime::Codex,
            "copilot" => Runtime::Copilot,
            _ => Runtime::ClaudeCode, // default fallback
        };
        assert_eq!(runtime, Runtime::ClaudeCode);
    }

    #[test]
    fn config_parse_max_concurrent_is_clamped() {
        // Test the clamping logic from App::new
        let val: usize = 100;
        let clamped = val.clamp(1, 20);
        assert_eq!(clamped, 20);

        let val: usize = 0;
        let clamped = val.clamp(1, 20);
        assert_eq!(clamped, 1);
    }

    #[test]
    fn config_round_trip() {
        let config = ObeliskConfig {
            orchestrator: Some(OrchestratorConfig {
                runtime: Some("claude".into()),
                max_concurrent: Some(5),
                auto_spawn: Some(true),
                poll_interval_secs: Some(60),
                velocity_window: Some(12),
            }),
            models: Some(ModelsConfig {
                claude: Some("claude-opus-4-6".into()),
                codex: Some("gpt-5.4".into()),
                copilot: Some("gpt-5".into()),
            }),
            theme: None,
            notifications: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: ObeliskConfig = toml::from_str(&toml_str).unwrap();

        let orch = restored.orchestrator.unwrap();
        assert_eq!(orch.runtime.as_deref(), Some("claude"));
        assert_eq!(orch.max_concurrent, Some(5));
        assert_eq!(orch.auto_spawn, Some(true));
        assert_eq!(orch.poll_interval_secs, Some(60));

        let models = restored.models.unwrap();
        assert_eq!(models.claude.as_deref(), Some("claude-opus-4-6"));
    }

    /// Config hot-reload tests are combined into a single test because they use
    /// `set_current_dir` which is process-global and would race with parallel tests.
    #[test]
    fn config_hot_reload() {
        let dir = std::env::temp_dir().join(format!("obelisk-test-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();

        // ── Part 1: detects orchestrator changes ──

        let initial = "[orchestrator]\nmax_concurrent = 5\nauto_spawn = false\npoll_interval_secs = 30\n";
        std::fs::write("obelisk.toml", initial).unwrap();

        let mut app = App::new();
        assert_eq!(app.max_concurrent, 5);
        assert!(!app.auto_spawn);
        assert_eq!(app.poll_interval_secs, 30);

        // No change yet
        app.frame_count = 100;
        assert!(!app.check_config_reload());

        // Write new config with different values (sleep to ensure mtime differs)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let updated = "[orchestrator]\nmax_concurrent = 12\nauto_spawn = true\npoll_interval_secs = 10\n";
        std::fs::write("obelisk.toml", updated).unwrap();
        app.frame_count = 200;

        assert!(app.check_config_reload());
        assert_eq!(app.max_concurrent, 12);
        assert!(app.auto_spawn);
        assert_eq!(app.poll_interval_secs, 10);

        // Second check without changes should return false
        app.frame_count = 300;
        assert!(!app.check_config_reload());

        // ── Part 2: theme change ──

        let original_primary = app.theme.primary;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write("obelisk.toml", "[theme]\npreset = \"nord\"\n").unwrap();
        app.frame_count = 400;
        assert!(app.check_config_reload());
        assert_ne!(app.theme.primary, original_primary);

        // ── Part 3: invalid TOML logs warning, preserves old values ──

        let mc_before = app.max_concurrent;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write("obelisk.toml", "[invalid toml!!!").unwrap();
        app.frame_count = 500;

        assert!(!app.check_config_reload());
        assert_eq!(app.max_concurrent, mc_before);
        let has_warning = app.event_log.iter().any(|e| e.message.contains("Config reload failed"));
        assert!(has_warning);

        // Cleanup
        std::env::set_current_dir(&orig_dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Config validation ───────────────────────────────────────

    #[test]
    fn validate_warns_on_unknown_top_level_key() {
        let toml_str = r#"
            [orchestrator]
            runtime = "claude"
            [bogus_section]
            foo = 1
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("bogus_section")));
    }

    #[test]
    fn validate_warns_on_unknown_orchestrator_key() {
        let toml_str = r#"
            [orchestrator]
            runtime = "claude"
            turbo_mode = true
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("orchestrator.turbo_mode")));
    }

    #[test]
    fn validate_warns_on_unknown_runtime() {
        let toml_str = r#"
            [orchestrator]
            runtime = "gemini"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("Unknown runtime 'gemini'")));
    }

    #[test]
    fn validate_warns_on_max_concurrent_zero() {
        let toml_str = r#"
            [orchestrator]
            max_concurrent = 0
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("max_concurrent=0")));
    }

    #[test]
    fn validate_warns_on_max_concurrent_too_high() {
        let toml_str = r#"
            [orchestrator]
            max_concurrent = 99
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("max_concurrent=99")));
    }

    #[test]
    fn validate_warns_on_poll_interval_zero() {
        let toml_str = r#"
            [orchestrator]
            poll_interval_secs = 0
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("poll_interval_secs")));
    }

    #[test]
    fn validate_warns_on_velocity_window_too_small() {
        let toml_str = r#"
            [orchestrator]
            velocity_window = 1
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("velocity_window=1")));
    }

    #[test]
    fn validate_warns_on_unknown_model() {
        let toml_str = r#"
            [models]
            claude = "nonexistent-model-v9"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("nonexistent-model-v9")));
    }

    #[test]
    fn validate_warns_on_unknown_theme_preset() {
        let toml_str = r#"
            [theme]
            preset = "nonexistent"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("nonexistent")));
    }

    #[test]
    fn validate_accepts_theme_preset_aliases() {
        for alias in &["frost", "ember", "ash", "deep", "dusk", "amber", "twilight", "carbon", "bloom", "moss"] {
            let toml_str = format!("[theme]\npreset = \"{}\"", alias);
            let config: ObeliskConfig = toml::from_str(&toml_str).unwrap();
            let warnings = validate_config(&toml_str, &config);
            let preset_warnings: Vec<_> = warnings.iter()
                .filter(|w| w.contains("Unknown theme preset"))
                .collect();
            assert!(preset_warnings.is_empty(),
                "Alias '{}' should be accepted but got: {:?}", alias, preset_warnings);
        }
    }

    #[test]
    fn validate_no_warnings_for_valid_config() {
        let toml_str = r#"
            [orchestrator]
            runtime = "claude"
            max_concurrent = 10
            auto_spawn = false
            poll_interval_secs = 30
            velocity_window = 24

            [models]
            claude = "claude-opus-4-6"
            codex = "gpt-5.4"
            copilot = "gpt-5"

            [theme]
            preset = "nord"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        // Filter out PATH-related warnings since those depend on the test environment
        let non_path: Vec<_> = warnings.iter()
            .filter(|w| !w.contains("not found on PATH"))
            .collect();
        assert!(non_path.is_empty(), "Unexpected warnings: {:?}", non_path);
    }

    #[test]
    fn validate_warns_on_unknown_models_key() {
        let toml_str = r#"
            [models]
            gemini = "gemini-pro"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("models.gemini")));
    }

    #[test]
    fn validate_warns_on_unknown_theme_key() {
        let toml_str = r#"
            [theme]
            background_image = "cats.png"
        "#;
        let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
        let warnings = validate_config(toml_str, &config);
        assert!(warnings.iter().any(|w| w.contains("theme.background_image")));
    }

    // ── Aggregate stats ──────────────────────────────────────────

    #[test]
    fn aggregate_stats_with_empty_history() {
        let mut app = App::new();
        app.history_sessions.clear();

        let (sessions, completed, failed, avg_dur, total_cost) = app.aggregate_stats();
        assert_eq!(sessions, 0);
        assert_eq!(completed, 0);
        assert_eq!(failed, 0);
        assert_eq!(avg_dur, 0.0);
        assert_eq!(total_cost, 0.0);
    }

    #[test]
    fn aggregate_stats_sums_across_sessions() {
        let mut app = App::new();
        app.history_sessions = vec![
            SessionRecord {
                session_id: "s1".into(),
                started_at: "2026-03-12T10:00:00Z".into(),
                ended_at: "2026-03-12T11:00:00Z".into(),
                total_completed: 3,
                total_failed: 1,
                total_cost_usd: 1.50,
                agents: vec![SessionAgent {
                    task_id: "t".into(), runtime: "CLAUDE".into(), model: "m".into(),
                    elapsed_secs: 100, status: "Completed".into(),
                    input_tokens: 1000, output_tokens: 500, estimated_cost_usd: 1.50,
                }],
            },
            SessionRecord {
                session_id: "s2".into(),
                started_at: "2026-03-12T12:00:00Z".into(),
                ended_at: "2026-03-12T13:00:00Z".into(),
                total_completed: 2,
                total_failed: 0,
                total_cost_usd: 2.00,
                agents: vec![SessionAgent {
                    task_id: "t".into(), runtime: "CLAUDE".into(), model: "m".into(),
                    elapsed_secs: 200, status: "Completed".into(),
                    input_tokens: 2000, output_tokens: 1000, estimated_cost_usd: 2.00,
                }],
            },
        ];

        let (sessions, completed, failed, avg_dur, total_cost) = app.aggregate_stats();
        assert_eq!(sessions, 2);
        assert_eq!(completed, 5);
        assert_eq!(failed, 1);
        assert!((avg_dur - 150.0).abs() < f64::EPSILON); // (100+200)/2
        assert!((total_cost - 3.50).abs() < f64::EPSILON);
    }

    // ── Dep graph ────────────────────────────────────────────────

    #[test]
    fn rebuild_dep_graph_flat_list() {
        let mut app = App::new();
        app.dep_graph_nodes = vec![
            DepNode { id: "a".into(), title: "A".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "b".into(), title: "B".into(), status: "open".into(),
                      priority: Some(2), issue_type: None, depth: 0, parent_id: None, truncated: false },
        ];

        app.rebuild_dep_graph_rows();

        assert_eq!(app.dep_graph_rows.len(), 2);
    }

    #[test]
    fn rebuild_dep_graph_with_parent_child() {
        let mut app = App::new();
        app.dep_graph_nodes = vec![
            DepNode { id: "root".into(), title: "Root".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "child".into(), title: "Child".into(), status: "open".into(),
                      priority: Some(2), issue_type: None, depth: 1, parent_id: Some("root".into()), truncated: false },
        ];

        app.rebuild_dep_graph_rows();

        assert_eq!(app.dep_graph_rows.len(), 2);
        assert!(app.dep_graph_rows[0].has_children);
    }

    #[test]
    fn dep_graph_collapse_hides_children() {
        let mut app = App::new();
        app.dep_graph_nodes = vec![
            DepNode { id: "root".into(), title: "Root".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "child".into(), title: "Child".into(), status: "open".into(),
                      priority: Some(2), issue_type: None, depth: 1, parent_id: Some("root".into()), truncated: false },
        ];

        // Collapse root
        app.dep_graph_collapsed.insert("root".into());
        app.rebuild_dep_graph_rows();

        assert_eq!(app.dep_graph_rows.len(), 1, "child should be hidden when root is collapsed");
        assert!(app.dep_graph_rows[0].collapsed);
    }

    // ── Dependency-aware auto-spawn ─────────────────────────────

    #[test]
    fn auto_spawn_skips_tasks_with_unclosed_dep_graph_parent() {
        let mut app = App::new();
        app.auto_spawn = true;
        app.max_concurrent = 5;

        // Task B depends on task A (parent_id points to A). A is still open.
        app.ready_tasks = vec![
            BeadTask { id: "B".into(), title: "Child".into(), status: "open".into(),
                       priority: Some(1), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.dep_graph_nodes = vec![
            DepNode { id: "A".into(), title: "Parent".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "B".into(), title: "Child".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 1, parent_id: Some("A".into()), truncated: false },
        ];

        // B has an unclosed parent A — should be skipped
        let result = app.get_auto_spawn_info();
        assert!(result.is_none(), "should not spawn B when parent A is not closed");
    }

    #[test]
    fn auto_spawn_allows_task_when_dep_graph_parent_is_closed() {
        let mut app = App::new();
        app.auto_spawn = true;
        app.max_concurrent = 5;

        app.ready_tasks = vec![
            BeadTask { id: "B".into(), title: "Child".into(), status: "open".into(),
                       priority: Some(1), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.dep_graph_nodes = vec![
            DepNode { id: "A".into(), title: "Parent".into(), status: "closed".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "B".into(), title: "Child".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 1, parent_id: Some("A".into()), truncated: false },
        ];

        // A is closed — B should be eligible
        let result = app.get_auto_spawn_info();
        assert!(result.is_some(), "should spawn B when parent A is closed");
    }

    #[test]
    fn auto_spawn_skips_blocked_tasks() {
        let mut app = App::new();
        app.auto_spawn = true;
        app.max_concurrent = 5;

        app.ready_tasks = vec![
            BeadTask { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                       priority: Some(1), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.blocked_tasks = vec![
            BlockedTask {
                task: BeadTask { id: "X".into(), title: "Blocked".into(), status: "blocked".into(),
                                 priority: Some(1), issue_type: None, assignee: None,
                                 labels: None, description: None, created_at: None },
                remaining_deps: 1,
            },
        ];

        let result = app.get_auto_spawn_info();
        assert!(result.is_none(), "should not spawn a blocked task");
    }

    #[test]
    fn auto_spawn_picks_eligible_task_over_blocked_one() {
        let mut app = App::new();
        app.auto_spawn = true;
        app.max_concurrent = 5;

        // Two tasks: X is blocked (P1), Y is free (P2)
        app.ready_tasks = vec![
            BeadTask { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                       priority: Some(1), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
            BeadTask { id: "Y".into(), title: "Free".into(), status: "open".into(),
                       priority: Some(2), issue_type: None, assignee: None,
                       labels: None, description: None, created_at: None },
        ];
        app.dep_graph_nodes = vec![
            DepNode { id: "dep".into(), title: "Dep".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
            DepNode { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                      priority: Some(1), issue_type: None, depth: 1, parent_id: Some("dep".into()), truncated: false },
        ];

        // X has unclosed dep, Y is free — should pick Y even though X has higher priority
        let result = app.get_auto_spawn_info();
        assert!(result.is_some(), "should spawn the eligible task Y");
        let spawned = result.unwrap();
        assert_eq!(spawned.task.id, "Y", "should have spawned Y, not blocked X");
    }

    // ── Search helpers ───────────────────────────────────────────

    #[test]
    fn compute_search_matches_finds_text() {
        let mut parser = vt100::Parser::new(5, 20, 10);
        parser.process(b"hello world\r\nhello again\r\n");
        let matches = compute_search_matches(parser.screen(), "hello");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn compute_search_matches_is_case_insensitive() {
        let mut parser = vt100::Parser::new(5, 20, 10);
        parser.process(b"Hello HELLO\r\n");
        let matches = compute_search_matches(parser.screen(), "hello");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn compute_search_matches_empty_query_returns_empty() {
        let parser = vt100::Parser::new(5, 20, 10);
        let matches = compute_search_matches(parser.screen(), "");
        assert!(matches.is_empty());
    }

    #[test]
    fn search_next_wraps_around() {
        let mut app = App::new();
        app.search_matches = vec![(0, 0), (1, 0), (2, 0)];
        app.search_current_idx = 2;

        app.search_next();
        assert_eq!(app.search_current_idx, 0);
    }

    #[test]
    fn search_prev_wraps_around() {
        let mut app = App::new();
        app.search_matches = vec![(0, 0), (1, 0), (2, 0)];
        app.search_current_idx = 0;

        app.search_prev();
        assert_eq!(app.search_current_idx, 2);
    }

    // ── Failure context extraction ───────────────────────────────

    #[test]
    fn extract_failure_context_includes_exit_code() {
        let mut agent = test_agent(1, AgentStatus::Failed);
        agent.exit_code = Some(1);
        agent.output = VecDeque::from(vec!["line 1".to_string(), "line 2".to_string()]);

        let ctx = extract_failure_context(&agent, 10);
        assert!(ctx.contains("Exit code: 1"));
    }

    #[test]
    fn extract_failure_context_limits_output_lines() {
        let mut agent = test_agent(1, AgentStatus::Failed);
        agent.exit_code = Some(1);
        for i in 0..100 {
            agent.output.push_back(format!("line {}", i));
        }

        let ctx = extract_failure_context(&agent, 5);
        assert!(ctx.contains("Last 5 lines"));
        assert!(ctx.contains("line 99")); // should include the last line
        assert!(!ctx.contains("line 0"));  // should NOT include the first line
    }

    // ── Active task IDs ──────────────────────────────────────────

    #[test]
    fn active_task_ids_only_returns_active_agents() {
        let mut app = App::new();
        let mut a1 = test_agent(1, AgentStatus::Running);
        a1.task.id = "running-task".into();
        let mut a2 = test_agent(2, AgentStatus::Completed);
        a2.task.id = "done-task".into();
        app.agents = vec![a1, a2];

        let ids = app.active_task_ids();
        assert!(ids.contains("running-task"));
        assert!(!ids.contains("done-task"));
    }

    // ── Diff result handling ─────────────────────────────────────

    #[test]
    fn on_diff_result_accepted_when_viewing_same_agent() {
        let mut app = App::new();
        app.show_diff_panel = true;
        app.selected_agent_id = Some(1);

        let diff = DiffData {
            lines: vec!["+ new line".into()],
            files_changed: 1,
            insertions: 1,
            deletions: 0,
            changed_files: vec!["src/main.rs".into()],
        };

        app.on_diff_result(1, diff);
        assert!(app.diff_data.is_some());
    }

    #[test]
    fn on_diff_result_rejected_when_viewing_different_agent() {
        let mut app = App::new();
        app.show_diff_panel = true;
        app.selected_agent_id = Some(1);

        let diff = DiffData {
            lines: vec![], files_changed: 0, insertions: 0, deletions: 0, changed_files: vec![],
        };

        app.on_diff_result(2, diff); // Different agent
        assert!(app.diff_data.is_none());
    }

    #[test]
    fn toggle_diff_panel_resets_state() {
        let mut app = App::new();
        app.show_diff_panel = false;
        app.diff_scroll = 5;

        app.toggle_diff_panel();
        assert!(app.show_diff_panel);
        assert_eq!(app.diff_scroll, 0);

        app.toggle_diff_panel();
        assert!(!app.show_diff_panel);
        assert!(app.diff_data.is_none());
    }

    // ── Sync PTY sizes ───────────────────────────────────────────

    #[test]
    fn sync_pty_sizes_ignores_too_small() {
        let mut app = App::new();
        app.last_pty_size = (24, 120);

        app.sync_pty_sizes(1, 5); // Too small
        assert_eq!(app.last_pty_size, (24, 120), "should not change for tiny sizes");
    }

    #[test]
    fn sync_pty_sizes_skips_if_unchanged() {
        let mut app = App::new();
        app.last_pty_size = (24, 120);

        app.sync_pty_sizes(24, 120); // Same
        // No-op: just verify it doesn't panic
    }

    // ── Recent completions ───────────────────────────────────────

    #[test]
    fn recent_completions_capped_at_10() {
        let mut app = App::new();
        for i in 0..15 {
            app.recent_completions.push_back(CompletionRecord {
                task_id: format!("t-{}", i),
                title: "T".into(),
                runtime: "CLAUDE".into(),
                model: "m".into(),
                elapsed_secs: 60,
                success: true,
            });
            if app.recent_completions.len() > 10 {
                app.recent_completions.pop_front();
            }
        }
        assert_eq!(app.recent_completions.len(), 10);
    }

    // ── Merge queue ────────────────────────────────────────────

    fn test_agent_with_task(id: usize, status: AgentStatus, task_id: &str) -> AgentInstance {
        AgentInstance {
            id,
            unit_number: id,
            task: BeadTask {
                id: task_id.into(),
                title: format!("task {}", task_id),
                status: "in_progress".into(),
                priority: None,
                issue_type: None,
                assignee: None,
                labels: None,
                description: None,
                created_at: None,
            },
            runtime: Runtime::ClaudeCode,
            model: "test-model".into(),
            status,
            phase: AgentPhase::Verifying,
            output: VecDeque::new(),
            started_at: std::time::Instant::now(),
            elapsed_secs: 0,
            exit_code: None,
            pid: None,
            retry_count: 0,
            worktree_path: None,
            worktree_cleaned: false,
            template_name: String::new(),
            pinned_to_split: None,
            total_lines: 0,
            raw_pty_log: Vec::new(),
            pty_log_flushed_bytes: 0,
            started_at_utc: chrono::Utc::now(),
            usage: None,
        }
    }

    #[test]
    fn merge_queue_enqueues_on_merging_phase() {
        let mut app = App::new();
        app.agents.push(test_agent_with_task(10, AgentStatus::Running, "obelisk-abc"));

        // Simulate PTY data with --no-ff (triggers Merging phase)
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");

        assert_eq!(app.merge_queue.len(), 1);
        assert_eq!(app.merge_queue[0].agent_id, 10);
        assert_eq!(app.merge_queue[0].task_id, "obelisk-abc");
        // Should log the merge entry
        assert!(app.event_log.iter().any(|e| e.message.contains("MERGE-QUEUE") && e.message.contains("obelisk-abc")));
    }

    #[test]
    fn merge_queue_does_not_duplicate_enqueue() {
        let mut app = App::new();
        app.agents.push(test_agent_with_task(10, AgentStatus::Running, "obelisk-abc"));

        // First trigger — should enqueue
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
        assert_eq!(app.merge_queue.len(), 1);

        // Second trigger with same phase — should not duplicate
        app.on_agent_pty_data(10, b"some other --no-ff text");
        assert_eq!(app.merge_queue.len(), 1);
    }

    #[test]
    fn merge_queue_dequeues_on_closing_phase() {
        let mut app = App::new();
        let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
        agent.phase = AgentPhase::Verifying;
        app.agents.push(agent);

        // Enter merging phase
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
        assert_eq!(app.merge_queue.len(), 1);

        // Advance to closing phase — should dequeue
        app.on_agent_pty_data(10, b"bd close obelisk-abc --reason done");
        assert_eq!(app.merge_queue.len(), 0);
        assert!(app.event_log.iter().any(|e| e.message.contains("merge complete")));
    }

    #[test]
    fn merge_queue_dequeues_on_agent_exit() {
        let mut app = App::new();
        let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
        agent.phase = AgentPhase::Verifying;
        app.agents.push(agent);

        // Enter merging phase
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
        assert_eq!(app.merge_queue.len(), 1);

        // Agent exits (e.g. crash) — should dequeue
        app.on_agent_exited(10, Some(1));
        assert_eq!(app.merge_queue.len(), 0);
        assert!(app.event_log.iter().any(|e| e.message.contains("exited while merging")));
    }

    #[test]
    fn merge_queue_logs_conflict_detection() {
        let mut app = App::new();
        let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
        agent.phase = AgentPhase::Verifying;
        app.agents.push(agent);

        // Enter merging phase
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");

        // Simulate conflict output during merge
        app.on_agent_pty_data(10, b"CONFLICT (content): Merge conflict in src/main.rs");
        assert!(app.event_log.iter().any(|e| e.message.contains("CONFLICT") && e.message.contains("obelisk-abc")));
    }

    #[test]
    fn merge_queue_multiple_agents_tracks_position() {
        let mut app = App::new();
        let mut agent1 = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
        agent1.phase = AgentPhase::Verifying;
        let mut agent2 = test_agent_with_task(11, AgentStatus::Running, "obelisk-def");
        agent2.phase = AgentPhase::Verifying;
        app.agents.push(agent1);
        app.agents.push(agent2);

        // Agent 1 enters merge phase
        app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
        assert_eq!(app.merge_queue.len(), 1);
        // Should log "proceeding" (queue was empty)
        assert!(app.event_log.iter().any(|e| e.message.contains("proceeding")));

        // Agent 2 enters merge phase while agent 1 is still merging
        app.on_agent_pty_data(11, b"git merge def --no-ff -m \"msg\"");
        assert_eq!(app.merge_queue.len(), 2);
        // Should log "1 agent(s) ahead"
        assert!(app.event_log.iter().any(|e| e.message.contains("1 agent(s) ahead")));

        // Agent 1 finishes merge (advances to closing)
        app.on_agent_pty_data(10, b"bd close obelisk-abc --reason done");
        assert_eq!(app.merge_queue.len(), 1);
        assert_eq!(app.merge_queue[0].agent_id, 11);
    }
}
