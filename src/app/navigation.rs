use super::*;

impl App {
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
}
