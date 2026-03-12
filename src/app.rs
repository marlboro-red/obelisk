use crate::types::*;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet, VecDeque};

const AGENT_PROMPT_TEMPLATE: &str = r#"# Beads Agent Prompt — Worktree Workflow

You are an autonomous coding agent. You will be given a beads issue ID to work on.
Your workflow is: **claim → worktree → implement → verify → merge → close**.

Every `bd` command MUST use the `--json` flag for structured output.

**CRITICAL: NEVER make code changes directly on the default branch (main/master).
ALL implementation work MUST happen in a worktree. The only changes on the default
branch should be the merge commit from Phase 5.**

---

## Phase 0: Detect Project Conventions

Before starting, determine the default branch and how to run tests/lint:

```bash
# Detect default branch (master or main)
DEFAULT_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@')
if [ -z "$DEFAULT_BRANCH" ]; then
  DEFAULT_BRANCH=$(git branch -l main master --format '%(refname:short)' | head -1)
fi

# Detect test/lint commands by inspecting project files
# Look at: Makefile, package.json, Cargo.toml, pyproject.toml, .github/workflows, etc.
# Use whatever the project already uses — do NOT guess.
```

Use `$DEFAULT_BRANCH` everywhere below instead of hardcoding a branch name.

---

## Phase 1: Claim the Issue

```bash
git checkout $DEFAULT_BRANCH
git pull --rebase

# Read the issue — understand scope, acceptance criteria, dependencies
bd show {id} --json

# Claim it (sets status to in_progress and assigns to you)
bd update {id} --claim --json

# Commit the beads state change before creating worktree
git add .beads/
git commit -m "claim {id}"
```

If the issue has unresolved blockers (`blocked_by` in the output), STOP and report
back — do not proceed on a blocked issue.

---

## Phase 2: Create a Git Worktree

Work in an isolated worktree so the default branch stays clean and other agents are unaffected.

```bash
BRANCH="{id}"
git worktree add "../worktree-${{BRANCH}}" -b "${{BRANCH}}" "$DEFAULT_BRANCH"
cd "../worktree-${{BRANCH}}"

# Verify bd can see the issue from the worktree
bd show {id} --json
```

If `bd show` fails to find the database, set up a redirect to the main repo's `.beads`:

```bash
mkdir -p .beads
echo "../../$(basename $(pwd -P | xargs dirname))/.beads" > .beads/redirect
```

---

## Phase 3: Implement

1. **Understand before changing.** Read relevant source files, tests, and docs first.
2. **Make focused commits.** Include the issue ID in every commit message:
   ```
   git commit -m "<description> ({id})"
   ```
3. **Discover new work.** If you find bugs or follow-ups, file them:
   ```bash
   bd create "Description" -t bug -p 2 --deps discovered-from:{id} --json
   ```
4. **Update progress notes.** Record context for future agents:
   ```bash
   bd update {id} --notes "COMPLETED: <what>. IN PROGRESS: <what>. DECISIONS: <why>." --json
   ```
5. **Do NOT use `bd edit`** — it opens an interactive editor which agents cannot use.

---

## Phase 4: Verify Against the Issue

Re-read the issue and confirm every detail has been addressed:

```bash
bd show {id} --json
```

Walk through the issue's description, acceptance criteria, and any linked context.
For each requirement, verify the corresponding change exists in your commits:

```bash
git log --oneline $DEFAULT_BRANCH..HEAD
git diff $DEFAULT_BRANCH --stat
```

If anything is missing or only partially implemented, go back to Phase 3.
Do NOT proceed to merge until the issue is fully addressed — not "mostly done."

---

## Phase 5: Merge

```bash
cd -   # back to main repo
git checkout $DEFAULT_BRANCH
git pull --rebase

# Merge the feature branch
git merge "{id}" --no-ff -m "Merge {id}: <short summary>"

# For .beads/*.jsonl merge conflicts:
#   git checkout --theirs .beads/issues.jsonl && bd import -i .beads/issues.jsonl

# Run the project's test and lint commands (discovered in Phase 0)
```

---

## Phase 6: Close the Issue

```bash
bd close {id} --reason "Completed: <specific summary of deliverables>" --json

# Commit the beads state change
git add .beads/
git commit -m "close {id}"
```

---

## Phase 7: Verify Completion

```bash
bd show {id} --json   # should show status: closed
git log --oneline $DEFAULT_BRANCH~3..$DEFAULT_BRANCH   # should show your merge commit
```

---

## Error Recovery

| Problem | Action |
|---|---|
| Tests fail after merge | Fix on the default branch, amend merge commit, re-run tests |
| `.beads/` merge conflicts | `git checkout --theirs .beads/issues.jsonl` then `bd import -i .beads/issues.jsonl` |
| `bd` can't find database in worktree | Set up `.beads/redirect` per Phase 2 |
| Issue is blocked | STOP. Report back. Do not work on blocked issues |
| Already claimed by another agent | Run `bd ready --json` and pick different work |
"#;

pub struct SpawnRequest {
    pub task: BeadTask,
    pub runtime: Runtime,
    pub model: String,
    pub agent_id: usize,
    pub system_prompt: String,
    pub user_prompt: String,
    pub pty_rows: u16,
    pub pty_cols: u16,
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
    /// None = auto-follow (pinned to bottom), Some(n) = manual scroll at line n from top
    pub agent_output_scroll: Option<usize>,

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

    // PTY state per agent: terminal parser + writer + master
    pub pty_states: HashMap<usize, PtyHandle>,

    // Interactive terminal mode — keystrokes go to the agent's PTY
    pub interactive_mode: bool,

    // Last known PTY inner area (rows, cols) — used to avoid redundant resizes
    pub last_pty_size: (u16, u16),

    // Help overlay toggle
    pub show_help: bool,
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
            agent_output_scroll: None,
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
            pty_states: HashMap::new(),
            interactive_mode: false,
            last_pty_size: (24, 120),
            show_help: false,
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
        // Auto-detach interactive mode if this agent was the one we were attached to
        if self.interactive_mode && self.selected_agent_id == Some(agent_id) {
            self.interactive_mode = false;
        }

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
            pty_rows: self.last_pty_size.0,
            pty_cols: self.last_pty_size.1,
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
                let max_scroll = self.event_log.len();
                if self.log_scroll < max_scroll {
                    self.log_scroll += 1;
                }
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
                    self.agent_output_scroll = None;
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
            format!("AGENT-{:02} terminated (killed)", unit),
        );
        Some(unit)
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

    /// Store PTY handle for an agent (called when AgentPtyReady arrives).
    /// Also responds to ConPTY's initial ESC[6n (Device Status Report) query —
    /// without this response, ConPTY buffers all child output indefinitely.
    pub fn on_agent_pty_ready(&mut self, agent_id: usize, mut handle: PtyHandle) {
        use std::io::Write;
        // ConPTY sends ESC[6n on startup; respond with cursor at (1,1)
        let _ = handle.writer.write_all(b"\x1b[1;1R");
        let _ = handle.writer.flush();
        self.pty_states.insert(agent_id, handle);
    }

    /// Feed raw PTY bytes into the agent's vt100 parser.
    pub fn on_agent_pty_data(&mut self, agent_id: usize, data: &[u8]) {
        if let Some(state) = self.pty_states.get_mut(&agent_id) {
            state.parser.process(data);
        }
        // Transition Starting → Running on first data received
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
            if agent.status == AgentStatus::Starting {
                agent.status = AgentStatus::Running;
            }
        }
        // Also count for throughput tracking (approximate: count newlines)
        let newlines = data.iter().filter(|&&b| b == b'\n').count() as u16;
        self.lines_this_tick = self.lines_this_tick.saturating_add(newlines.max(1));
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
            state.parser.screen_mut().set_size(rows, cols);
        }
    }

    /// Count output lines for an agent. Uses the vt100 screen if a PTY is
    /// active, otherwise falls back to the legacy line buffer.
    pub fn agent_line_count(&self, agent_id: usize) -> usize {
        if let Some(state) = self.pty_states.get(&agent_id) {
            let screen = state.parser.screen();
            let (rows, _cols) = screen.size();
            // Count non-empty rows from the bottom up to find the last used row
            let mut last_used = 0;
            for row in 0..rows {
                let text = screen.contents_between(row, 0, row, screen.size().1);
                if !text.trim().is_empty() {
                    last_used = row as usize + 1;
                }
            }
            // Add scrollback: contents above the visible screen
            let scrollback = screen.scrollback();
            last_used + scrollback
        } else if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            agent.output.len()
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
