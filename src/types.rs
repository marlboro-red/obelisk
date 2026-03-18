use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Priority,
    Type,
    Age,
    Name,
}

impl SortMode {
    pub fn label(&self) -> &'static str {
        match self {
            SortMode::Priority => "priority",
            SortMode::Type => "type",
            SortMode::Age => "age",
            SortMode::Name => "name",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SortMode::Priority => SortMode::Type,
            SortMode::Type => SortMode::Age,
            SortMode::Age => SortMode::Name,
            SortMode::Name => SortMode::Priority,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct BeadTask {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub issue_type: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

impl BeadTask {
    /// Returns true if this issue is an epic (a container for child issues).
    /// Epics should never be directly assigned to agents — only their children.
    pub fn is_epic(&self) -> bool {
        self.issue_type.as_deref() == Some("epic")
    }
}

/// A dependency record returned by `bd list -s blocked --json`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct BeadDependency {
    pub issue_id: String,
    pub depends_on_id: String,
    #[serde(rename = "type")]
    pub dep_type: String,
}

/// A blocked issue with its unresolved dependency info.
#[derive(Debug, Clone)]
pub struct BlockedTask {
    pub task: BeadTask,
    /// Number of unresolved (non-closed) blocked-by dependencies
    pub remaining_deps: usize,
}

/// Raw deserialization target for `bd list -s blocked --json` entries.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct BlockedTaskRaw {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub issue_type: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<BeadDependency>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runtime {
    ClaudeCode,
    Codex,
    Copilot,
}

impl Runtime {
    pub fn name(&self) -> &str {
        match self {
            Runtime::ClaudeCode => "CLAUDE",
            Runtime::Codex => "CODEX",
            Runtime::Copilot => "COPILOT",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Runtime::ClaudeCode => Runtime::Codex,
            Runtime::Codex => Runtime::Copilot,
            Runtime::Copilot => Runtime::ClaudeCode,
        }
    }

    pub fn models(&self) -> &'static [&'static str] {
        match self {
            Runtime::ClaudeCode => &[
                "claude-sonnet-4-6",
                "claude-opus-4-6",
                "claude-haiku-4-5-20251001",
            ],
            Runtime::Codex => &[
                "gpt-5.4",
                "gpt-5.3-codex",
                "gpt-5.3-codex-spark",
            ],
            Runtime::Copilot => &["claude-sonnet-4", "gpt-5"],
        }
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

/// Tracks which phase of the 7-phase agent workflow is currently executing.
/// Detected heuristically by scanning PTY output for phase-indicating patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentPhase {
    Detecting,
    Claiming,
    Worktree,
    Implementing,
    Verifying,
    Merging,
    Closing,
    Done,
}

impl AgentPhase {
    pub fn label(&self) -> &'static str {
        match self {
            AgentPhase::Detecting => "Detecting",
            AgentPhase::Claiming => "Claiming",
            AgentPhase::Worktree => "Worktree",
            AgentPhase::Implementing => "Implementing",
            AgentPhase::Verifying => "Verifying",
            AgentPhase::Merging => "Merging",
            AgentPhase::Closing => "Closing",
            AgentPhase::Done => "Done",
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            AgentPhase::Detecting => "P0",
            AgentPhase::Claiming => "P1",
            AgentPhase::Worktree => "P2",
            AgentPhase::Implementing => "P3",
            AgentPhase::Verifying => "P4",
            AgentPhase::Merging => "P5",
            AgentPhase::Closing => "P6",
            AgentPhase::Done => "P7",
        }
    }

}

impl AgentStatus {
    pub fn symbol(&self) -> &str {
        match self {
            AgentStatus::Starting => "◐",
            AgentStatus::Running => "▶",
            AgentStatus::Completed => "✓",
            AgentStatus::Failed => "✗",
        }
    }
}

/// Filter for the agent list panel. Cycles through All → Running → Failed → Completed → Starting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatusFilter {
    All,
    Running,
    Failed,
    Completed,
    Starting,
}

impl AgentStatusFilter {
    pub fn next(&self) -> Self {
        match self {
            AgentStatusFilter::All => AgentStatusFilter::Running,
            AgentStatusFilter::Running => AgentStatusFilter::Failed,
            AgentStatusFilter::Failed => AgentStatusFilter::Completed,
            AgentStatusFilter::Completed => AgentStatusFilter::Starting,
            AgentStatusFilter::Starting => AgentStatusFilter::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AgentStatusFilter::All => "ALL",
            AgentStatusFilter::Running => "▶ RUNNING",
            AgentStatusFilter::Failed => "✗ FAILED",
            AgentStatusFilter::Completed => "✓ DONE",
            AgentStatusFilter::Starting => "◐ INIT",
        }
    }

    pub fn matches(&self, status: AgentStatus) -> bool {
        match self {
            AgentStatusFilter::All => true,
            AgentStatusFilter::Running => status == AgentStatus::Running,
            AgentStatusFilter::Failed => status == AgentStatus::Failed,
            AgentStatusFilter::Completed => status == AgentStatus::Completed,
            AgentStatusFilter::Starting => status == AgentStatus::Starting,
        }
    }
}

#[derive(Debug)]
pub struct AgentInstance {
    pub id: usize,
    pub unit_number: usize,
    pub task: BeadTask,
    pub runtime: Runtime,
    pub model: String,
    pub status: AgentStatus,
    pub phase: AgentPhase,
    pub output: VecDeque<String>,
    pub started_at: std::time::Instant,
    pub elapsed_secs: u64,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    pub retry_count: u32,
    /// Relative path to the agent's git worktree, e.g. "../worktree-obelisk-abc"
    pub worktree_path: Option<String>,
    /// True once the worktree has been cleaned up (removed + branch deleted)
    pub worktree_cleaned: bool,
    /// Which template was used (e.g. "bug.md" or "feature.md (built-in)")
    pub template_name: String,
    /// Whether this agent is pinned to a split-pane slot
    pub pinned_to_split: Option<usize>,
    /// Running total of output lines (newlines received), survives PTY resizes
    pub total_lines: usize,
    /// Raw PTY bytes captured for log export
    pub raw_pty_log: Vec<u8>,
    /// Number of raw_pty_log bytes already flushed to disk
    pub pty_log_flushed_bytes: usize,
    /// Wall-clock time when the agent was spawned (for matching Claude Code sessions)
    pub started_at_utc: chrono::DateTime<chrono::Utc>,
    /// Token usage read from Claude Code session logs after completion
    pub usage: Option<AgentUsage>,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub category: LogCategory,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCategory {
    System,
    Incoming,
    Deploy,
    Complete,
    Alert,
    Poll,
}

impl LogCategory {
    pub fn label(&self) -> &str {
        match self {
            LogCategory::System => "SYSTEM",
            LogCategory::Incoming => "INCOMING",
            LogCategory::Deploy => "DEPLOY",
            LogCategory::Complete => "COMPLETE",
            LogCategory::Alert => "ALERT",
            LogCategory::Poll => "POLL",
        }
    }
}

/// A completed issue entry for the recent completions feed on the Dashboard.
#[derive(Debug, Clone)]
pub struct CompletionRecord {
    pub task_id: String,
    pub title: String,
    pub runtime: String,
    pub model: String,
    pub elapsed_secs: u64,
    pub success: bool,
}

/// Token and cost usage data extracted from Claude Code session logs.
#[derive(Debug, Clone, Default)]
pub struct AgentUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
}

/// One agent's outcome within a persisted session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAgent {
    pub task_id: String,
    pub runtime: String,
    pub model: String,
    pub elapsed_secs: u64,
    pub status: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

/// Entry in the merge queue — tracks an agent that has entered the Merging phase.
#[derive(Debug, Clone)]
pub struct MergeQueueEntry {
    /// Agent instance ID
    pub agent_id: usize,
    /// Agent unit number (for AGENT-NN display)
    pub unit_number: usize,
    /// Task/issue ID being merged
    pub task_id: String,
    /// When the agent entered the merge queue
    pub enqueued_at: std::time::Instant,
}

/// A single session record appended to `.beads/obelisk_sessions.jsonl` on exit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub total_completed: u32,
    pub total_failed: u32,
    #[serde(default)]
    pub total_cost_usd: f64,
    pub agents: Vec<SessionAgent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    AgentDetail,
    EventLog,
    History,
    SplitPane,
    WorktreeOverview,
    DepGraph,
}

/// A node in the dependency graph, parsed from `bd dep tree --json`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct DepNode {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub issue_type: Option<String>,
    #[serde(default)]
    pub depth: usize,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// A flattened row for rendering the dep graph tree view.
#[derive(Debug, Clone)]
pub struct DepGraphRow {
    pub node: DepNode,
    pub collapsed: bool,
    pub has_children: bool,
}

/// Status classification for a worktree entry in the overview panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeStatus {
    /// An agent is currently running on this worktree
    Active,
    /// Worktree exists but no agent is running on it
    Idle,
    /// No matching agent or issue — candidate for cleanup
    Orphaned,
}

/// A single worktree entry enriched with agent and issue linkage.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    /// Absolute path to the worktree directory
    pub path: String,
    /// Git branch name
    pub branch: String,
    /// Linked issue ID parsed from worktree-{id} naming
    pub issue_id: Option<String>,
    /// Associated agent ID (if any)
    pub agent_id: Option<usize>,
    /// Status classification
    pub status: WorktreeStatus,
    /// Creation time (from filesystem metadata)
    pub created_at: Option<chrono::DateTime<chrono::Local>>,
}

/// Sort mode for the worktree overview panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeSortMode {
    Age,
    Status,
}

impl WorktreeSortMode {
    pub fn next(&self) -> Self {
        match self {
            WorktreeSortMode::Age => WorktreeSortMode::Status,
            WorktreeSortMode::Status => WorktreeSortMode::Age,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            WorktreeSortMode::Age => "age",
            WorktreeSortMode::Status => "status",
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ReadyQueue,
    BlockedQueue,
    AgentList,
}

/// Holds PTY master + writer + terminal parser for an agent.
/// Created in the spawn task, sent to the main thread via AgentPtyReady.
pub struct PtyHandle {
    /// Kept alive to prevent PTY close — not read directly.
    #[allow(dead_code)]
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub parser: vt100::Parser,
    /// Tracks the previous scrollback value so we can detect growth.
    pub prev_scrollback: usize,
    /// Cumulative scrollback lines that have ever scrolled off the visible area.
    pub cumulative_scrollback: usize,
    /// Cumulative newline count from raw PTY data — platform-independent fallback
    /// for line counting when vt100 scrollback doesn't work (e.g. Windows ConPTY).
    pub cumulative_newlines: usize,
}

/// Parsed git diff data for an agent's worktree.
#[derive(Debug, Clone)]
pub struct DiffData {
    /// Raw diff lines (including +/- prefixes)
    pub lines: Vec<String>,
    /// Summary: number of files changed
    pub files_changed: usize,
    /// Summary: total insertions
    pub insertions: usize,
    /// Summary: total deletions
    pub deletions: usize,
    /// List of changed file paths
    pub changed_files: Vec<String>,
}

/// Stores layout rectangles computed during rendering, used for mouse hit-testing.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct LayoutAreas {
    pub tab_bar: Option<ratatui::layout::Rect>,
    pub ready_queue: Option<ratatui::layout::Rect>,
    pub blocked_queue: Option<ratatui::layout::Rect>,
    pub agent_panel: Option<ratatui::layout::Rect>,
    pub agent_detail_output: Option<ratatui::layout::Rect>,
    pub split_panes: [Option<ratatui::layout::Rect>; 4],
}

/// State for the issue creation form overlay.
#[derive(Debug, Clone)]
pub struct IssueCreationForm {
    pub title: String,
    pub description: String,
    pub issue_type_idx: usize,
    pub priority: i32,
    pub focused_field: usize, // 0=title, 1=description, 2=type, 3=priority
}

/// Available issue types for the creation form.
pub const ISSUE_TYPES: &[&str] = &["feature", "bug", "task", "chore", "epic"];

impl IssueCreationForm {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            issue_type_idx: 0,
            priority: 2,
            focused_field: 0,
        }
    }

    pub fn issue_type(&self) -> &'static str {
        ISSUE_TYPES[self.issue_type_idx]
    }
}

pub enum AppEvent {
    Terminal(crossterm::event::Event),
    Tick,
    PollResult(Vec<BeadTask>),
    /// Poll failed — carries the error message for display and logging
    PollFailed(String),
    AgentOutput { agent_id: usize, line: String },
    AgentExited { agent_id: usize, exit_code: Option<i32> },
    AgentPid { agent_id: usize, pid: u32 },
    /// Raw bytes from PTY output — fed into vt100 parser on main thread
    AgentPtyData { agent_id: usize, data: Vec<u8> },
    /// PTY is ready — carries the master/writer/parser to store in App
    AgentPtyReady { agent_id: usize, handle: Box<PtyHandle> },
    /// Orphaned agent worktrees found on startup
    WorktreeOrphans(Vec<String>),
    /// Result of a worktree cleanup operation
    WorktreeCleaned { cleaned: Vec<String>, failed: Vec<String> },
    /// Result of a git diff poll for an agent's worktree
    DiffResult { agent_id: usize, diff: DiffData },
    /// Result of a worktree scan for the overview panel
    WorktreeScanned(Vec<(String, String)>),
    /// Result of a dependency graph poll
    DepGraphResult(Vec<DepNode>),
    /// Dependency graph poll failed
    DepGraphFailed(String),
    /// Result of a blocked-issues poll
    BlockedPollResult(Vec<BlockedTask>),
    /// Result of issue creation via `bd create`
    IssueCreateResult(Result<String, String>),
    /// An agent's beads issue has been polled and found to be closed
    IssueStatusClosed { agent_id: usize },
}
