use super::*;

impl App {
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
}
