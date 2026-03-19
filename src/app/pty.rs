use super::*;

impl App {
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
            // visible text (^[[1;1R]), so only send it on Windows.
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
                if let Some(entry) = self.merge_queue.remove(pos) {
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

        // Drain flushed bytes from memory to bound growth (obelisk-ufp).
        // The data is safely on disk now, so we can release the memory.
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.raw_pty_log.drain(..new_bytes);
            agent.pty_log_flushed_bytes = 0;
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
        // Since flushed bytes are drained from memory (obelisk-ufp), read from
        // the persisted log file on disk which contains the complete history.
        content.push_str("\n════════════════════════════════════════════════════════════════════════════════\n");
        content.push_str("RAW PTY OUTPUT\n");
        content.push_str("════════════════════════════════════════════════════════════════════════════════\n\n");

        let persisted_log = PathBuf::from(".obelisk")
            .join("logs")
            .join(format!("agent-{:02}-{}.log", agent.unit_number, agent.task.id));
        let disk_data = std::fs::read(&persisted_log).ok();
        let has_disk = disk_data.as_ref().map_or(false, |d| !d.is_empty());
        let has_mem = !agent.raw_pty_log.is_empty();

        if !has_disk && !has_mem {
            content.push_str("(No raw PTY data captured)\n");
        } else {
            if has_disk {
                content.push_str(&String::from_utf8_lossy(disk_data.as_ref().unwrap()));
            }
            // Append any unflushed bytes still in memory
            if has_mem {
                content.push_str(&String::from_utf8_lossy(&agent.raw_pty_log));
            }
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
