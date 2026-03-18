use super::*;

impl App {
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
                            &self.webhook_failure_tx,
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
}
