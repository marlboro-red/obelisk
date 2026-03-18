use super::*;

impl App {
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
                        AgentStatus::Killing => "Killing".to_string(),
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
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running | AgentStatus::Killing))
            .count()
    }

    pub fn count_running(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running | AgentStatus::Killing))
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
            if matches!(agent.status, AgentStatus::Starting | AgentStatus::Running | AgentStatus::Killing) {
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

        // Surface webhook delivery failures to the event log
        while let Ok(msg) = self.webhook_failure_rx.try_recv() {
            self.log(LogCategory::Alert, msg);
        }

        // Periodically flush PTY logs to disk for running agents (~every 30s)
        if self.frame_count.is_multiple_of(300) {
            let running_ids: Vec<usize> = self
                .agents
                .iter()
                .filter(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running | AgentStatus::Killing))
                .map(|a| a.id)
                .collect();
            for id in running_ids {
                self.persist_agent_pty_log(id);
            }
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
}

/// Return the path to the sessions JSONL file. Tries `.beads/obelisk_sessions.jsonl`
/// relative to the current working directory (where the binary is run from).
pub(super) fn sessions_file_path() -> std::path::PathBuf {
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
pub(super) fn generate_session_id() -> String {
    let now = chrono::Local::now();
    format!("sess-{}", now.format("%Y%m%d-%H%M%S"))
}
