use super::*;

impl App {
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
}
