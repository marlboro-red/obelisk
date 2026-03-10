use crate::types::*;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet, VecDeque};

const AGENT_PROMPT_TEMPLATE: &str = r#"Work on beads issue {id}. Follow this workflow exactly.

## Phase 1: Claim the Issue on Master

```bash
git checkout master
git pull --rebase
bd show {id} --json
bd update {id} --claim --json
```

If the issue has unresolved blockers (`blocked_by` in the output), STOP and report back — do not proceed on a blocked issue.

## Phase 2: Create a Git Worktree

Work in an isolated worktree so master stays clean and other agents are unaffected.

```bash
BRANCH="{id}"
git worktree add "../worktree-${BRANCH}" -b "${BRANCH}" master
cd "../worktree-${BRANCH}"
bd show {id} --json
```

If `bd show` fails to find the database, set up a redirect:

```bash
mkdir -p .beads
echo "../../$(basename $(pwd -P | xargs dirname))/.beads" > .beads/redirect
```

## Phase 3: Implement

1. **Understand before changing.** Read relevant source files, tests, and docs first.
2. **Make focused commits.** Include the issue ID in every commit message:
   ```
   git commit -m "feat: <description> ({id})"
   ```
3. **Discover new work.** If you find bugs or follow-ups, file them:
   ```bash
   bd create "Description" -t bug -p 2 --deps discovered-from:{id} --json
   ```
4. **Update progress notes:**
   ```bash
   bd update {id} --notes "COMPLETED: <what>. IN PROGRESS: <what>. DECISIONS: <why>." --json
   ```
5. **Do NOT use `bd edit`** — it opens an interactive editor which agents cannot use.

## Phase 4: Verify Against the Issue

Re-read the issue and confirm every detail has been addressed:

```bash
bd show {id} --json
git log --oneline master..HEAD
git diff master --stat
```

Walk through the issue description and acceptance criteria. If anything is missing or only partially implemented, go back to Phase 3. Do NOT proceed to merge until the issue is fully addressed.

## Phase 5: Merge into Master

```bash
cd -
git checkout master
git pull --rebase
git merge "{id}" --no-ff -m "Merge {id}: <short summary>"
make test
make lint
```

For `.beads/*.jsonl` merge conflicts: `git checkout --theirs .beads/issues.jsonl && bd import -i .beads/issues.jsonl`

## Phase 6: Close & Push

The plane is NOT landed until `git push` succeeds. Do NOT stop before pushing.

```bash
bd close {id} --reason "Completed: <specific summary of deliverables>" --json
git pull --rebase
git push
git status
bd dolt push 2>/dev/null || true
```

## Phase 7: Verify Completion

```bash
bd show {id} --json   # should show status: closed
git status             # should show "up to date with origin/master"
```

## Error Recovery

- Tests fail after merge: Fix on master, amend merge commit, re-run tests
- `.beads/` merge conflicts: `git checkout --theirs .beads/issues.jsonl` then `bd import -i .beads/issues.jsonl`
- `bd` can't find database in worktree: Set up `.beads/redirect` per Phase 2
- Issue is blocked: STOP. Report back. Do not work on blocked issues
- Already claimed by another agent: Run `bd ready --json` and pick different work
"#;

pub struct SpawnRequest {
    pub task: BeadTask,
    pub runtime: Runtime,
    pub model: String,
    pub agent_id: usize,
    pub system_prompt: String,
    pub user_prompt: String,
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

    pub selected_agent_id: Option<usize>,
    pub agent_output_scroll: usize,

    pub prompt_template: String,

    pub frame_count: u64,
    pub wave_offset: f64,

    // Throughput tracking (lines per second over last 60 ticks)
    pub throughput_history: VecDeque<u16>,
    pub lines_this_tick: u16,

    // Alert system
    pub alert_message: Option<(String, u64)>, // (message, frame_to_expire)

    // Per-runtime model selection
    pub model_indices: HashMap<Runtime, usize>,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            ready_tasks: Vec::new(),
            agents: Vec::new(),
            event_log: VecDeque::with_capacity(500),
            active_view: View::Dashboard,
            focus: Focus::ReadyQueue,
            task_list_state: ListState::default(),
            agent_list_state: ListState::default(),
            log_scroll: 0,
            selected_runtime: Runtime::ClaudeCode,
            auto_spawn: false,
            max_concurrent: 10,
            poll_interval_secs: 30,
            poll_countdown: 30.0,
            should_quit: false,
            next_unit: 0,
            claimed_task_ids: HashSet::new(),
            total_completed: 0,
            total_failed: 0,
            selected_agent_id: None,
            agent_output_scroll: 0,
            prompt_template: AGENT_PROMPT_TEMPLATE.to_string(),
            frame_count: 0,
            wave_offset: 0.0,
            throughput_history: VecDeque::from(vec![0; 60]),
            lines_this_tick: 0,
            alert_message: None,
            model_indices: HashMap::from([
                (Runtime::ClaudeCode, 0),
                (Runtime::Codex, 0),
                (Runtime::Copilot, 0),
            ]),
        };
        app.log(LogCategory::System, "Orchestrator initialized".into());
        app.log(LogCategory::System, "System online".into());
        app
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
    }

    pub fn on_poll_result(&mut self, tasks: Vec<BeadTask>) {
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
        }
        self.ready_tasks = new_tasks;
        self.log(
            LogCategory::Poll,
            format!(
                "Scan complete: {} ready, {} active",
                self.ready_tasks.len(),
                self.active_agent_count()
            ),
        );
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
        let log_info = if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.exit_code = exit_code;
            agent.elapsed_secs = agent.started_at.elapsed().as_secs();
            let unit = agent.unit_number;
            let task_id = agent.task.id.clone();
            let rt = agent.runtime.name().to_string();
            if exit_code == Some(0) {
                agent.status = AgentStatus::Completed;
                Some((true, unit, task_id, rt))
            } else {
                agent.status = AgentStatus::Failed;
                Some((false, unit, task_id, rt))
            }
        } else {
            None
        };

        if let Some((success, unit, task_id, rt)) = log_info {
            if success {
                self.total_completed += 1;
                self.log(
                    LogCategory::Complete,
                    format!("AGENT-{:02} completed {} [{}]", unit, task_id, rt),
                );
            } else {
                self.total_failed += 1;
                self.log(
                    LogCategory::Alert,
                    format!(
                        "AGENT-{:02} FAILED on {} [exit: {:?}]",
                        unit, task_id, exit_code
                    ),
                );
            }
        }
    }

    pub fn selected_task(&self) -> Option<&BeadTask> {
        self.task_list_state
            .selected()
            .and_then(|i| self.ready_tasks.get(i))
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

        let system_prompt = self
            .prompt_template
            .replace("{id}", &task.id)
            .replace("{title}", &task.title);
        let user_prompt = format!(
            "Work on beads issue {}. Follow the workflow in your system prompt exactly.",
            task.id
        );

        let agent = AgentInstance {
            id: unit,
            unit_number: unit,
            task: task.clone(),
            runtime,
            status: AgentStatus::Starting,
            output: VecDeque::new(),
            started_at: std::time::Instant::now(),
            elapsed_secs: 0,
            exit_code: None,
            pid: None,
        };

        self.claimed_task_ids.insert(task.id.clone());
        self.agents.push(agent);
        self.log(
            LogCategory::Deploy,
            format!(
                "AGENT-{:02} deployed on {} [{}/{}]",
                unit,
                task.id,
                runtime.name(),
                model
            ),
        );

        self.ready_tasks.retain(|t| t.id != task.id);
        if let Some(sel) = self.task_list_state.selected() {
            if sel >= self.ready_tasks.len() && !self.ready_tasks.is_empty() {
                self.task_list_state.select(Some(self.ready_tasks.len() - 1));
            } else if self.ready_tasks.is_empty() {
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
        })
    }

    pub fn get_auto_spawn_info(&mut self) -> Option<SpawnRequest> {
        if !self.auto_spawn
            || self.active_agent_count() >= self.max_concurrent
            || self.ready_tasks.is_empty()
        {
            return None;
        }
        self.task_list_state.select(Some(0));
        self.get_spawn_info()
    }

    pub fn navigate_up(&mut self) {
        match self.active_view {
            View::Dashboard => match self.focus {
                Focus::ReadyQueue => {
                    let len = self.ready_tasks.len();
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
                    let len = self.agents.len();
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
                if self.agent_output_scroll > 0 {
                    self.agent_output_scroll -= 1;
                }
            }
            View::EventLog => {
                if self.log_scroll > 0 {
                    self.log_scroll -= 1;
                }
            }
        }
    }

    pub fn navigate_down(&mut self) {
        match self.active_view {
            View::Dashboard => match self.focus {
                Focus::ReadyQueue => {
                    let len = self.ready_tasks.len();
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
                    let len = self.agents.len();
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
                        let max_scroll = agent.output.len();
                        if self.agent_output_scroll < max_scroll {
                            self.agent_output_scroll += 1;
                        }
                    }
                }
            }
            View::EventLog => {
                let max_scroll = self.event_log.len();
                if self.log_scroll < max_scroll {
                    self.log_scroll += 1;
                }
            }
        }
    }

    pub fn toggle_focus(&mut self) {
        if self.active_view == View::Dashboard {
            self.focus = match self.focus {
                Focus::ReadyQueue => Focus::AgentList,
                Focus::AgentList => Focus::ReadyQueue,
            };
        }
    }

    pub fn enter_pressed(&mut self) {
        if self.active_view == View::Dashboard && self.focus == Focus::AgentList {
            if let Some(sel) = self.agent_list_state.selected() {
                if sel < self.agents.len() {
                    self.selected_agent_id = Some(self.agents[sel].id);
                    self.agent_output_scroll = 0;
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

    /// Kill agent process via SIGTERM. Returns unit number for logging.
    pub fn kill_agent(&mut self, agent_id: usize) -> Option<usize> {
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
        self.total_failed += 1;
        self.log(
            LogCategory::Alert,
            format!("AGENT-{:02} terminated (SIGTERM)", unit),
        );
        Some(unit)
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
}
