use crate::templates;
use crate::types::*;
use ratatui::widgets::ListState;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::LazyLock;

// All valid issue types for cycling the type filter
const ALL_TYPES: &[&str] = &["bug", "feature", "task", "chore", "epic"];

// ── Token parsing regexes ──
static RE_INPUT_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)input[_ ]tokens?[:\s]+([0-9][0-9,]*)").unwrap()
});
static RE_OUTPUT_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)output[_ ]tokens?[:\s]+([0-9][0-9,]*)").unwrap()
});

fn parse_token_count(s: &str) -> u64 {
    s.replace(',', "").parse::<u64>().unwrap_or(0)
}

fn default_model_pricing() -> HashMap<String, ModelPricing> {
    let mut m = HashMap::new();
    m.insert("claude-sonnet-4-6".into(), ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 });
    m.insert("claude-opus-4-6".into(), ModelPricing { input_per_mtok: 15.0, output_per_mtok: 75.0 });
    m.insert("claude-haiku-4-5-20251001".into(), ModelPricing { input_per_mtok: 0.80, output_per_mtok: 4.0 });
    m.insert("claude-sonnet-4".into(), ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 });
    m.insert("gpt-5.4".into(), ModelPricing { input_per_mtok: 10.0, output_per_mtok: 30.0 });
    m.insert("gpt-5.3-codex".into(), ModelPricing { input_per_mtok: 6.0, output_per_mtok: 18.0 });
    m.insert("gpt-5.3-codex-spark".into(), ModelPricing { input_per_mtok: 3.0, output_per_mtok: 9.0 });
    m.insert("gpt-5".into(), ModelPricing { input_per_mtok: 10.0, output_per_mtok: 30.0 });
    m
}


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
    pub wave_offset: f64,

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

    /// When true, auto-send /exit to ClaudeCode agents when completion is detected
    pub auto_exit_on_completion: bool,

    // Diff panel state
    pub show_diff_panel: bool,
    pub diff_data: Option<DiffData>,
    pub diff_scroll: usize,
    /// Frame count at which the last diff poll was triggered
    pub diff_last_poll_frame: u64,

    // Desktop notifications
    pub notifications_enabled: bool,

    // Split-pane view state
    /// Agent IDs pinned to each pane slot (up to 4)
    pub split_pane_agents: [Option<usize>; 4],
    /// Which pane (0-3) is currently focused
    pub split_pane_focus: usize,
    /// Output scroll per pane
    pub split_pane_scroll: [Option<usize>; 4],

    // Cost tracking
    pub model_pricing: HashMap<String, ModelPricing>,
    pub cost_threshold: Option<f64>,

    /// Layout rectangles from last render — used for mouse hit-testing
    pub layout_areas: LayoutAreas,

    /// Whether mouse support is enabled (toggle with 'M' on dashboard)
    pub mouse_enabled: bool,

    // Jump-to-issue mode
    pub jump_active: bool,
    pub jump_query: String,

    // Velocity sparkline — configurable window (number of data points)
    pub velocity_window_size: usize,

    // Kill confirmation dialog — Some(agent_id) means dialog is visible
    pub confirm_kill_agent_id: Option<usize>,

    // Worktree overview panel state
    pub worktree_entries: Vec<WorktreeEntry>,
    pub worktree_list_state: ListState,
    pub worktree_sort_mode: WorktreeSortMode,
    /// Frame count at which the last worktree scan was triggered
    pub worktree_last_scan_frame: u64,
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

impl App {
    pub fn new() -> Self {
        let config_exists = std::path::Path::new(CONFIG_FILE).exists();
        let config = if config_exists {
            std::fs::read_to_string(CONFIG_FILE)
                .ok()
                .and_then(|s| toml::from_str::<ObeliskConfig>(&s).ok())
                .unwrap_or_default()
        } else {
            ObeliskConfig::default()
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
                poll_interval_secs = pi;
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

        let poll_countdown = poll_interval_secs as f64;
        let mut app = Self {
            ready_tasks: Vec::new(),
            agents: Vec::new(),
            event_log: VecDeque::with_capacity(500),
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
            wave_offset: 0.0,
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
            auto_exit_on_completion: true,
            show_diff_panel: false,
            diff_data: None,
            diff_scroll: 0,
            diff_last_poll_frame: 0,
            notifications_enabled: true,
            split_pane_agents: [None; 4],
            split_pane_focus: 0,
            split_pane_scroll: [None; 4],
            model_pricing: default_model_pricing(),
            cost_threshold: Some(5.0),
            layout_areas: LayoutAreas::default(),
            mouse_enabled: true,
            jump_active: false,
            jump_query: String::new(),
            velocity_window_size,
            confirm_kill_agent_id: None,
            worktree_entries: Vec::new(),
            worktree_list_state: ListState::default(),
            worktree_sort_mode: WorktreeSortMode::Age,
            worktree_last_scan_frame: 0,
        };
        app.log(LogCategory::System, "Orchestrator initialized".into());
        if config_exists {
            app.log(LogCategory::System, format!("Config loaded from {}", CONFIG_FILE));
        } else {
            app.log(LogCategory::System, "No config file found, using defaults".into());
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
        };
        match toml::to_string_pretty(&config) {
            Ok(toml_str) => {
                if std::fs::write(CONFIG_FILE, toml_str).is_ok() {
                    self.log(LogCategory::System, format!("Config saved to {}", CONFIG_FILE));
                }
            }
            Err(_) => {}
        }
    }

    /// Build a SessionRecord from the current session and append it to the
    /// persistent log file `.beads/obelisk_sessions.jsonl`.
    pub fn save_session(&self) {
        let ended_at = chrono::Local::now();
        let agents: Vec<SessionAgent> = self
            .agents
            .iter()
            .map(|a| SessionAgent {
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
                input_tokens: a.input_tokens,
                output_tokens: a.output_tokens,
                estimated_cost_usd: a.estimated_cost_usd,
            })
            .collect();

        let record = SessionRecord {
            session_id: self.session_id.clone(),
            started_at: self.session_started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            ended_at: ended_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            total_completed: self.total_completed,
            total_failed: self.total_failed,
            total_cost_usd: self.session_total_cost(),
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

    pub fn on_tick(&mut self) {
        self.frame_count += 1;
        self.wave_offset = (self.wave_offset + 0.15) % (std::f64::consts::TAU * 100.0);
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
        if self.frame_count % 10 == 0 {
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

        // Auto-exit timeout: force-complete agents that sent /exit but did not exit
        let timeout = std::time::Duration::from_secs(10);
        let timed_out: Vec<usize> = self
            .agents
            .iter()
            .filter(|a| {
                a.completion_detected
                    && matches!(a.status, AgentStatus::Starting | AgentStatus::Running)
                    && a.exit_sent_at
                        .map(|t| t.elapsed() > timeout)
                        .unwrap_or(false)
            })
            .map(|a| a.id)
            .collect();
        for id in timed_out {
            self.force_complete_agent(id);
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
            .collect();
        let new_count = new_tasks
            .iter()
            .filter(|t| !self.ready_tasks.iter().any(|rt| rt.id == t.id))
            .count();
        if new_count > 0 {
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
                let high_prio_ids: Vec<&str> = new_tasks
                    .iter()
                    .filter(|t| {
                        !self.ready_tasks.iter().any(|rt| rt.id == t.id)
                            && t.priority.unwrap_or(3) <= 1
                    })
                    .map(|t| t.id.as_str())
                    .collect();
                if !high_prio_ids.is_empty() {
                    crate::notify::send_notification(
                        "High-Priority Task Available",
                        &format!("P0/P1 ready: {}", high_prio_ids.join(", ")),
                    );
                    crate::notify::send_bell();
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

        let log_info = if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.exit_code = exit_code;
            agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            let unit = agent.unit_number;
            let task_id = agent.task.id.clone();
            let rt = agent.runtime.name().to_string();
            let elapsed = agent.elapsed_secs;
            if exit_code == Some(0) {
                agent.status = AgentStatus::Completed;
                Some((true, unit, task_id, rt, elapsed))
            } else {
                agent.status = AgentStatus::Failed;
                Some((false, unit, task_id, rt, elapsed))
            }
        } else {
            None
        };

        if let Some((success, unit, task_id, rt, elapsed)) = log_info {
            if success {
                self.total_completed += 1;
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
                }
            } else {
                self.total_failed += 1;
                self.log(
                    LogCategory::Alert,
                    format!(
                        "AGENT-{:02} FAILED on {} [exit: {:?}]",
                        unit, task_id, exit_code
                    ),
                );
                if self.notifications_enabled {
                    crate::notify::send_notification(
                        "Agent Failed",
                        &format!("AGENT-{:02} \u{00b7} {} failed [exit: {:?}]", unit, task_id, exit_code),
                    );
                    crate::notify::send_bell();
                }
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
            completion_detected: false,
            exit_sent_at: None,
            completion_buf: String::new(),
            pinned_to_split: None,
            input_tokens: 0,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
            template_name: template_name.clone(),
            total_lines: 0,
        };

        self.claimed_task_ids.insert(task.id.clone());
        self.agents.push(agent);
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
        // Respect filters — only auto-spawn from the visible filtered set.
        // The filtered set is already sorted by priority (highest first).
        if self.filtered_tasks().is_empty() {
            return None;
        }
        self.task_list_state.select(Some(0));
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
                let max_scroll = self.event_log.len();
                if self.log_scroll < max_scroll {
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
                Focus::ReadyQueue => Focus::AgentList,
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

    /// Kill agent process via SIGTERM. Returns (unit_number, worktree_path) for logging/cleanup.
    pub fn kill_agent(&mut self, agent_id: usize) -> Option<(usize, Option<String>)> {
        let agent = self.agents.iter_mut().find(|a| a.id == agent_id)?;
        if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
            return None;
        }
        if let Some(pid) = agent.pid {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            #[cfg(windows)]
            {
                use std::process::Command;
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        agent.status = AgentStatus::Failed;
        agent.elapsed_secs = agent.started_at.elapsed().as_secs();
        let unit = agent.unit_number;
        let worktree = agent.worktree_path.clone();
        self.total_failed += 1;
        self.log(
            LogCategory::Alert,
            format!("AGENT-{:02} terminated (killed)", unit),
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
            completion_detected: false,
            exit_sent_at: None,
            completion_buf: String::new(),
            pinned_to_split: None,
            input_tokens: 0,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
            template_name: template_name.clone(),
            total_lines: 0,
        };

        // task ID remains in claimed_task_ids (the new agent owns it)
        self.agents.push(new_agent);
        self.selected_agent_id = Some(unit);
        self.agent_output_scroll = None;

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
                    .map(|t| chrono::DateTime::<chrono::Local>::from(t));

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

    fn sort_worktree_entries(&self, entries: &mut Vec<WorktreeEntry>) {
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
    /// Also responds to ConPTY's initial ESC[6n (Device Status Report) query —
    /// without this response, ConPTY buffers all child output indefinitely.
    pub fn on_agent_pty_ready(&mut self, agent_id: usize, mut handle: PtyHandle) {
        use std::io::Write;
        // ConPTY sends ESC[6n on startup; respond with cursor at (1,1)
        let _ = handle.writer.write_all(b"\x1b[1;1R");
        let _ = handle.writer.flush();
        self.pty_states.insert(agent_id, handle);
    }

    /// Feed raw PTY bytes into the agent's vt100 parser.
    pub fn on_agent_pty_data(&mut self, agent_id: usize, data: &[u8]) {
        if let Some(state) = self.pty_states.get_mut(&agent_id) {
            state.parser.process(data);
        }
        // Transition Starting → Running on first data received, and detect phase
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            if agent.status == AgentStatus::Starting {
                agent.status = AgentStatus::Running;
            }
            // Phase detection — heuristic, best-effort; only advance, never retreat
            if let Ok(text) = std::str::from_utf8(data) {
                if let Some(detected) = detect_phase(text) {
                    if detected > agent.phase {
                        agent.phase = detected;
                    }
                }
                // Token usage parsing
                if let Some(caps) = RE_INPUT_TOKENS.captures(text) {
                    if let Some(m) = caps.get(1) {
                        agent.input_tokens = parse_token_count(m.as_str());
                    }
                }
                if let Some(caps) = RE_OUTPUT_TOKENS.captures(text) {
                    if let Some(m) = caps.get(1) {
                        agent.output_tokens = parse_token_count(m.as_str());
                    }
                }
            }
            // Track total output lines (newlines received) — survives PTY resizes
            agent.total_lines += data.iter().filter(|&&b| b == b'\n').count();
        }
        // Recalculate cost for this agent after potential token update
        self.recalculate_agent_cost(agent_id);
        // Also count for throughput tracking (approximate: count newlines)
        let newlines = data.iter().filter(|&&b| b == b'\n').count() as u16;
        self.lines_this_tick = self.lines_this_tick.saturating_add(newlines.max(1));

        // Completion detection: scan PTY output for beads issue closure markers
        if self.auto_exit_on_completion {
            self.check_completion_in_pty_data(agent_id, data);
        }
    }

    fn check_completion_in_pty_data(&mut self, agent_id: usize, data: &[u8]) {
        let agent = match self.agents.iter_mut().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return,
        };
        if agent.runtime != Runtime::ClaudeCode { return; }
        if !matches!(agent.status, AgentStatus::Running | AgentStatus::Starting) { return; }
        if agent.completion_detected { return; }

        let text = String::from_utf8_lossy(data);
        agent.completion_buf.push_str(&text);
        if agent.completion_buf.len() > 8192 {
            let excess = agent.completion_buf.len() - 8192;
            let drain_to = (excess..)
                .find(|&i| agent.completion_buf.is_char_boundary(i))
                .unwrap_or(excess);
            agent.completion_buf.drain(..drain_to);
        }

        let closed = agent.completion_buf.contains("\"status\": \"closed\"")
            || agent.completion_buf.contains("\"status\":\"closed\"")
            || agent.completion_buf.contains("status: closed");
        if !closed { return; }

        agent.completion_detected = true;
        agent.exit_sent_at = Some(std::time::Instant::now());
        let unit = agent.unit_number;

        self.log(
            LogCategory::Complete,
            format!("AGENT-{:02} auto-completed: detected issue closure", unit),
        );

        if let Some(state) = self.pty_states.get_mut(&agent_id) {
            use std::io::Write;
            let _ = state.writer.write_all(b"/exit
");
            let _ = state.writer.flush();
        }
    }

    fn force_complete_agent(&mut self, agent_id: usize) {
        let (unit, pid) = {
            let agent = match self.agents.iter_mut().find(|a| a.id == agent_id) {
                Some(a) => a,
                None => return,
            };
            if !matches!(agent.status, AgentStatus::Starting | AgentStatus::Running) {
                return;
            }
            agent.status = AgentStatus::Completed;
            agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            (agent.unit_number, agent.pid)
        };
        self.total_completed += 1;
        if let Some(pid) = pid {
            #[cfg(unix)]
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
            #[cfg(windows)]
            {
                use std::process::Command;
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        self.log(
            LogCategory::Alert,
            format!("AGENT-{:02} force-terminated after auto-exit timeout", unit),
        );
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
            // Replace the parser with a fresh screen at the new dimensions.
            // Calling set_size() reflows the existing content which can appear
            // garbled in the window between the resize and the child process
            // completing its SIGWINCH-triggered full repaint.  A blank parser
            // avoids that artifact; the child's redraw fills it in correctly.
            state.parser = vt100::Parser::new(rows, cols, 10000);
        }
    }

    /// Count total output lines for an agent. Uses the running `total_lines`
    /// counter (incremented on each newline received), which survives PTY
    /// resizes and is not capped by terminal dimensions.
    pub fn agent_line_count(&self, agent_id: usize) -> usize {
        if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            if agent.total_lines > 0 || self.pty_states.contains_key(&agent_id) {
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
        let completed = self.total_completed as f64;
        let total = self.agents.len() as f64;
        (completed / total * 100.0).min(100.0)
    }

    pub fn format_elapsed(secs: u64) -> String {
        let m = secs / 60;
        let s = secs % 60;
        format!("{:02}:{:02}", m, s)
    }

    /// Compute aggregate stats across all loaded history sessions.
    /// Returns (total_sessions, all_time_completed, all_time_failed, avg_duration_secs).
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

        let all_time_cost: f64 = self.history_sessions
            .iter()
            .map(|s| s.total_cost_usd)
            .sum();

        (total_sessions, all_time_completed, all_time_failed, avg_duration, all_time_cost)
    }

    /// Recalculate estimated cost for a single agent based on its token counts and model pricing.
    fn recalculate_agent_cost(&mut self, agent_id: usize) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            let pricing = self.model_pricing.get(&agent.model);
            let old_cost = agent.estimated_cost_usd;
            agent.estimated_cost_usd = if let Some(p) = pricing {
                (agent.input_tokens as f64 * p.input_per_mtok
                    + agent.output_tokens as f64 * p.output_per_mtok)
                    / 1_000_000.0
            } else {
                0.0
            };
            if let Some(threshold) = self.cost_threshold {
                if agent.estimated_cost_usd >= threshold && old_cost < threshold {
                    let unit = agent.unit_number;
                    let cost = agent.estimated_cost_usd;
                    let msg = format!(
                        "AGENT-{:02} cost ${:.2} exceeds threshold ${:.2}!",
                        unit, cost, threshold
                    );
                    self.alert_message = Some((msg.clone(), self.frame_count + 100));
                    self.log(LogCategory::Alert, msg);
                }
            }
        }
    }

    /// Compute velocity sparkline data: completed issues per session.
    /// Returns the last `velocity_window_size` data points, with the current
    /// session as the rightmost (most recent) point.
    pub fn velocity_sparkline_data(&self) -> Vec<u64> {
        let mut data: Vec<u64> = self
            .history_sessions
            .iter()
            .map(|s| s.total_completed as u64)
            .collect();
        // Current session as the live data point
        data.push(self.total_completed as u64);
        let skip = data.len().saturating_sub(self.velocity_window_size);
        data[skip..].to_vec()
    }

    /// Sum of estimated cost across all agents in the current session.
    pub fn session_total_cost(&self) -> f64 {
        self.agents.iter().map(|a| a.estimated_cost_usd).sum()
    }

    /// Sum of input + output tokens across all agents in the current session.
    pub fn session_total_tokens(&self) -> (u64, u64) {
        let input: u64 = self.agents.iter().map(|a| a.input_tokens).sum();
        let output: u64 = self.agents.iter().map(|a| a.output_tokens).sum();
        (input, output)
    }

    // ── Split-pane view methods ──

    /// Auto-populate split pane slots with running agents if not manually pinned.
    pub fn auto_fill_split_panes(&mut self) {
        let running: Vec<usize> = self
            .agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running))
            .map(|a| a.id)
            .collect();

        for slot in 0..4 {
            if let Some(id) = self.split_pane_agents[slot] {
                if self.agents.iter().any(|a| a.id == id && a.pinned_to_split == Some(slot)) {
                    continue;
                }
            }
            let assigned: Vec<Option<usize>> = self.split_pane_agents.to_vec();
            let candidate = running.iter().find(|&&id| !assigned.contains(&Some(id)));
            self.split_pane_agents[slot] = candidate.copied();
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
}

/// Format a USD cost for display.
pub fn format_cost(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else if usd < 100.0 {
        format!("${:.2}", usd)
    } else {
        format!("${:.0}", usd)
    }
}

/// Format a token count for display with K/M suffixes.
pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
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
