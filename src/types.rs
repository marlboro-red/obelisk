use serde::Deserialize;
use std::collections::VecDeque;

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

#[derive(Debug)]
pub struct AgentInstance {
    pub id: usize,
    pub unit_number: usize,
    pub task: BeadTask,
    pub runtime: Runtime,
    pub status: AgentStatus,
    pub output: VecDeque<String>,
    pub started_at: std::time::Instant,
    pub elapsed_secs: u64,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    AgentDetail,
    EventLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ReadyQueue,
    AgentList,
}

pub enum AppEvent {
    Terminal(crossterm::event::Event),
    Tick,
    PollResult(Vec<BeadTask>),
    AgentOutput { agent_id: usize, line: String },
    AgentExited { agent_id: usize, exit_code: Option<i32> },
    AgentPid { agent_id: usize, pid: u32 },
}
