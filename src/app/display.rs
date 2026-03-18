use super::*;

impl App {
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
}
