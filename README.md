# Obelisk

A terminal UI (TUI) orchestrator for managing autonomous AI coding agents. Obelisk monitors a [beads](https://github.com/anthropics/beads) issue queue, spawns AI agents to work on tasks in isolated git worktrees, and gives you real-time visibility into every agent's progress through a multi-view dashboard.

```
 ◈  O B E L I S K  ◈  BEADS ORCHESTRATOR  //  ● ONLINE
─────────────────────────────────────────────────────────
 1:DASHBOARD │ 2:AGENTS │ 3:EVENT LOG │ 4:HISTORY │ ...
─────────────────────────────────────────────────────────
 ┌─ READY QUEUE ──────────┐┌─ AGENTS ──────────────────┐
 │ P0 bug  obelisk-a1b    ││ AGENT-01 ▶ obelisk-x2y   │
 │ P1 task obelisk-c3d    ││   CLAUDE  P3:Implementing  │
 │ P2 feat obelisk-e5f    ││ AGENT-02 ✓ obelisk-z9w   │
 │                         ││   CODEX   P7:Done  3m42s  │
 └─────────────────────────┘└───────────────────────────┘
 ┌ THROUGHPUT ┐┌ VELOCITY ┐┌─ RECENT EVENTS ───────────┐
 │ ▂▃▅▇▅▃▂   ││ ▁▂▃▅▇   ││ 14:02:31 [DEPLOY] AGENT-01│
 └────────────┘└──────────┘└────────────────────────────┘
```

## Prerequisites

- **Rust** (1.70+) and **Cargo** for building
- **Git** with worktree support
- **bd** CLI ([beads](https://github.com/anthropics/beads)) for issue tracking
- At least one AI agent runtime:
  - [Claude Code](https://claude.ai/claude-code) (`claude` CLI)
  - [Codex](https://openai.com/codex) (`codex` CLI)
  - [GitHub Copilot](https://github.com/features/copilot) (`copilot` CLI)

## Installation

```bash
git clone <repo-url>
cd obelisk
cargo build --release
```

The binary is at `target/release/obelisk` (or `target/release/obelisk.exe` on Windows).

## Quick Start

1. Make sure you have a beads database initialized in your project (`bd init`).
2. Create some issues (`bd create "Fix the login bug" -t bug -p 1`).
3. Run obelisk from your project root:

```bash
cargo run --release
```

Obelisk will start polling `bd ready` for available tasks. Select a task and press `s` to spawn an agent, or enable auto-spawn mode with `a`.

## Views

Obelisk has seven views, accessible via number keys `1`-`7` or by clicking the tab bar.

### 1. Dashboard

The main operational view. Two panels side by side:

- **Ready Queue** (left) — Issues from `bd ready` sorted by priority. Shows issue ID, type badge, priority, title, and age.
- **Agent Panel** (right) — All spawned agents with status indicators, runtime, model, elapsed time, phase, and cost estimate.

Below the panels: real-time **throughput sparkline** (lines/sec), **velocity sparkline** (completions per session), and a **recent events** feed.

### 2. Agent Detail

Full-screen PTY output for the selected agent. Shows the agent's live terminal output rendered through a real vt100 parser with full ANSI color support.

Features:
- **Observe mode** — Watch the agent work with scrollable output
- **Interactive mode** — Press `i` to attach to the agent's PTY and type commands directly (press `Ctrl+]` to detach)
- **Search** — Press `/` to search the visible terminal buffer; `n`/`N` to cycle matches
- **Diff panel** — Press `d` to toggle a live git diff view of the agent's worktree
- **Log export** — Press `e` to export the agent's raw PTY log to a file

Header bar shows: agent ID, task ID, runtime, model, status, phase, elapsed time, token usage, and estimated cost.

### 3. Event Log

Scrollable log of all system events with timestamps and category badges:

| Category | Events |
|----------|--------|
| SYSTEM   | Config changes, orchestrator state |
| INCOMING | New tasks detected from poll |
| DEPLOY   | Agent spawned |
| COMPLETE | Agent finished successfully |
| ALERT    | Agent failures, poll errors |
| POLL     | Scan results, poll health |

Press `f` to cycle through category filters.

### 4. History

Session history loaded from `.beads/obelisk_sessions.jsonl`. Each row shows session ID, start/end times, completed/failed counts, total cost, and per-agent breakdowns. Sessions are persisted automatically when obelisk exits.

### 5. Split Pane

Monitor up to 4 agents simultaneously in a grid layout. Each pane shows a compact PTY output view. Panes auto-fill with running agents or can be manually pinned with `g`.

### 6. Worktree Overview

Lists all `worktree-*` directories with status classification:

| Status   | Meaning |
|----------|---------|
| Active   | An agent is currently running on this worktree |
| Idle     | Worktree exists but no agent is running |
| Orphaned | No matching agent or issue — candidate for cleanup |

Press `f` to toggle sort (age/status).

### 7. Dependency Graph

Tree view of all issues and their dependency relationships, parsed from `bd dep tree`. Expand/collapse subtrees with `Enter`. Shows status, priority, and type for each node.

## Agent Lifecycle

1. **Select** a task from the ready queue (or let auto-spawn pick one)
2. **Spawn** — Obelisk creates a PTY, launches the AI CLI with the appropriate prompt template, and begins streaming output
3. **Monitor** — Watch progress in the dashboard or agent detail view. Phase detection tracks the agent through: Claiming → Worktree → Implementing → Verifying → Merging → Closing → Done
4. **Retry** — If an agent fails, press `r` in agent detail to retry with failure context injected into the prompt
5. **Kill** — Press `k` to terminate a running agent (with confirmation dialog). The worktree is cleaned up automatically
6. **Dismiss** — Remove finished agents from the list with `x` (single) or `X` (all finished)

Agents run with `--dangerously-skip-permissions` (Claude Code) or equivalent flags for unattended operation. Each agent gets its own git worktree for isolation.

## Keybinding Reference

### Global

| Key | Action |
|-----|--------|
| `1`-`7` | Switch to view (Dashboard/Agents/EventLog/History/Split/Worktrees/Deps) |
| `?` | Toggle help overlay |
| `Ctrl+C` | Force quit |
| `n` | Toggle desktop notifications on/off |
| `+` / `-` | Increase / decrease max concurrent agent slots (1-20) |

### Dashboard

| Key | Action |
|-----|--------|
| `s` | Spawn agent on selected task |
| `p` | Trigger manual poll / scan |
| `r` | Cycle runtime (Claude → Codex → Copilot) |
| `m` | Cycle model for current runtime |
| `a` | Toggle auto-spawn mode |
| `c` | Scan and clean up orphaned worktrees |
| `w` | Open worktree overview |
| `/` | Jump to issue by ID |
| `Tab` | Toggle focus: Ready Queue ↔ Agent List |
| `Up`/`Down` or `j`/`k` | Navigate list |
| `Enter` | Open Agent Detail for selected agent |
| `y` | Copy issue ID to clipboard |
| `M` | Toggle mouse support on/off |

When focused on the **Ready Queue**:

| Key | Action |
|-----|--------|
| `f` | Cycle sort mode (priority → type → age → name) |
| `F` | Cycle type filter (bug/feature/task/chore/epic) |

When focused on the **Agent List**:

| Key | Action |
|-----|--------|
| `f` | Cycle agent status filter (All → Running → Failed → Done → Init) |
| `x` | Dismiss selected finished agent |
| `X` | Dismiss all finished agents |

### Agent Detail (Observe Mode)

| Key | Action |
|-----|--------|
| `i` | Attach interactive PTY session |
| `d` | Toggle live git diff panel |
| `J` / `K` | Scroll diff panel |
| `r` | Retry failed agent |
| `k` | Kill agent (with confirmation) |
| `e` | Export agent log to file |
| `Up`/`Down` | Scroll output one line |
| `PgUp`/`PgDn` | Scroll output by page |
| `Home` / `End` | Jump to top / re-engage auto-follow |
| `Left` / `Right` | Previous / next agent |
| `/` | Open search bar |
| `n` / `N` | Next / previous search match |
| `y` | Copy worktree path to clipboard |
| `Esc` / `q` | Return to Dashboard |

### Agent Detail (Interactive Mode)

| Key | Action |
|-----|--------|
| `Ctrl+]` | Detach from PTY (return to Observe mode) |
| *all other keys* | Forwarded to the agent's PTY |

### Event Log

| Key | Action |
|-----|--------|
| `Up`/`Down` | Scroll log |
| `f` | Cycle category filter (All → System → Incoming → Deploy → Complete → Alert → Poll) |

### History

| Key | Action |
|-----|--------|
| `Up`/`Down` | Scroll session list |
| `PgUp`/`PgDn` | Scroll by 10 sessions |

### Split Pane

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus between panes |
| `Up`/`Down` | Scroll focused pane output |
| `Enter` | Open Agent Detail for focused pane |
| `g` | Pin/unpin agent to focused pane |
| `Esc` / `q` | Return to Dashboard |

### Dependency Graph

| Key | Action |
|-----|--------|
| `Up`/`Down` or `j`/`k` | Navigate dependency list |
| `Enter` | Expand/collapse subtree |
| `Esc` / `q` | Return to Dashboard |

### Worktree Overview

| Key | Action |
|-----|--------|
| `Up`/`Down` or `j`/`k` | Navigate worktree list |
| `f` | Cycle sort mode (age/status) |
| `Esc` / `q` | Return to Dashboard |

### Mouse

| Action | Effect |
|--------|--------|
| Click | Select items in lists, switch tabs, focus split panes |
| Scroll | Navigate lists and scroll output |

Mouse support can be toggled with `M` on the Dashboard.

## Runtime and Model Selection

Obelisk supports three AI agent runtimes. Press `r` on the Dashboard to cycle between them, and `m` to cycle models within the selected runtime.

| Runtime | Models | Invocation |
|---------|--------|------------|
| Claude Code | claude-sonnet-4-6, claude-opus-4-6, claude-haiku-4-5 | `claude "<prompt>" --model <model> --dangerously-skip-permissions` |
| Codex | gpt-5.4, gpt-5.3-codex, gpt-5.3-codex-spark | `codex exec --dangerously-bypass-approvals-and-sandbox -m <model> "<prompt>"` |
| Copilot | claude-sonnet-4, gpt-5 | `copilot -p "<prompt>" --model <model> --yolo` |

Token usage and estimated costs are tracked per agent and displayed in the Agent Detail header and session history.

## Prompt Templates

Obelisk uses per-issue-type prompt templates to instruct agents. Templates are resolved in this order:

1. Custom template from `.obelisk/templates/<type>.md` (hot-reloaded from disk)
2. Built-in default template for the issue type

Built-in templates exist for: `bug`, `feature`, `task`, `chore`, `epic`.

Templates support variable interpolation: `{id}`, `{title}`, `{priority}`, `{description}`.

## Auto-Spawn Mode

When enabled (`a` to toggle), obelisk automatically spawns agents for ready tasks up to the configured concurrency limit. Combined with the polling interval, this creates a fully autonomous pipeline: issues arrive → agents spawn → work completes → results merge.

## Worktree Management

Each agent works in an isolated git worktree (`../worktree-<issue-id>`). Obelisk manages the full lifecycle:

- **Creation** — Worktrees are created automatically when agents spawn
- **Monitoring** — The Worktree Overview (view 6) shows all agent worktrees with status
- **Cleanup** — Press `c` on the Dashboard to scan and remove orphaned worktrees. Killing an agent also cleans up its worktree. On startup, obelisk warns about leftover worktrees from previous sessions

## Session Persistence

Session data is automatically saved to `.beads/obelisk_sessions.jsonl` when obelisk exits. Each record includes:

- Session ID and timestamps
- Per-agent: task ID, runtime, model, elapsed time, status, token usage, estimated cost
- Aggregate: total completed, total failed, total cost

Previous sessions are viewable in the History tab (view 4) with a velocity sparkline showing completions over time.

## Desktop Notifications

Obelisk sends native desktop notifications (via `notify-rust`) for:

- **High-priority tasks** — When P0/P1 tasks appear in the ready queue
- **Agent completion** — When an agent finishes successfully
- **Agent failure** — When an agent fails

A terminal bell (`\x07`) accompanies each notification. Toggle notifications with `n`.

## Configuration

Obelisk reads configuration from `obelisk.toml` in the project root. Settings are saved automatically on exit.

```toml
[orchestrator]
runtime = "claude"           # Default runtime: "claude", "codex", or "copilot"
max_concurrent = 10          # Max simultaneous agents (1-20)
auto_spawn = false           # Start in auto-spawn mode
poll_interval_secs = 30      # Seconds between bd ready polls
velocity_window = 24         # Number of sessions in velocity sparkline

[models]
claude = "claude-sonnet-4-6"             # Default model for Claude Code
codex = "gpt-5.4"                        # Default model for Codex
copilot = "claude-sonnet-4"              # Default model for Copilot
```

All settings can also be changed at runtime via keybindings and are persisted when obelisk exits.
