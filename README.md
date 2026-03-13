# Obelisk

Obelisk is a terminal orchestrator for autonomous coding agents. It watches a
[beads](https://github.com/anthropics/beads) issue queue, launches agent CLIs in
real PTYs, and gives you a live dashboard for ready work, blocked work,
worktrees, logs, and session history.

The current build supports both:

- an interactive TUI dashboard
- a headless daemon with CLI control commands

## Prerequisites

- Rust and Cargo
- Git with `worktree` support
- `bd` (beads CLI)
- At least one agent runtime on `PATH`:
  - `claude`
  - `codex`
  - `copilot`

## Installation

```bash
git clone <repo-url>
cd obelisk
cargo build --release
```

The binary will be available at `target/release/obelisk` (or
`target/release/obelisk.exe` on Windows).

## Quick Start

1. Start from a project that already has a beads database.
2. Create or sync some open issues.
3. Launch obelisk from that project root:

```bash
cargo run --release
```

Obelisk polls `bd ready --json` for runnable work and `bd list -s blocked
--json` for blocked issues. In the Dashboard, select a ready issue and press
`s` to spawn an agent, or press `a` to let obelisk auto-spawn up to the current
concurrency limit.

## CLI Modes

```bash
obelisk                 # Launch the TUI
obelisk serve           # Start the headless daemon
obelisk --daemon        # Alias for "serve"
obelisk status          # Show daemon status
obelisk agents          # List daemon-managed agents
obelisk spawn <issue>   # Ask the daemon to spawn a ready issue
obelisk kill <agent>    # Kill a running daemon-managed agent
obelisk stop            # Stop the daemon
```

Notes:

- `obelisk spawn <issue>` works against the daemon and expects the issue to
  already be in the ready queue.
- The daemon records its listener port in `.beads/obelisk.port`.

## Views

Obelisk has seven views, reachable with number keys `1` through `7`.

### 1. Dashboard

The operational overview. The current dashboard layout is:

- left column: ready queue, optional blocked queue, and a task detail preview
- right column: active/finished agent list
- bottom row: throughput chart, recent completions feed, and recent event log

On narrow terminals, the bottom row collapses and some side panels are hidden to
preserve PTY space.

### 2. Agent Detail

Full PTY view for the selected agent.

Features:

- observe mode with scrollback
- interactive attach mode with `i` and detach on `F2`
- live git diff panel with `d`
- PTY search with `/`, `n`, and `N`
- log export with `e`
- retry failed agent with `r`
- manual completion with `D`
- kill/cleanup with `k`

On wide terminals, a diagnostics sidebar shows runtime, priority, retries,
template name, worktree state, elapsed time, line count, and line rate. The
current build does not expose token or cost telemetry.

### 3. Event Log

System log with timestamped categories:

- `SYSTEM`
- `INCOMING`
- `DEPLOY`
- `COMPLETE`
- `ALERT`
- `POLL`

Use `f` to cycle the category filter.

### 4. History

Session history loaded from `.beads/obelisk_sessions.jsonl`.

The History view currently shows:

- all-time aggregate statistics
- a session log with session ID, start time, completed count, failed count,
  success rate, and agent count

### 5. Split Pane

Multi-agent monitor for live PTY output. Depending on terminal width, obelisk
shows:

- 2 panes at medium widths
- up to 4 panes on wide terminals

Use `g` to pin or unpin an agent in a pane.

### 6. Worktree Overview

Lists `worktree-*` directories and classifies them as:

- `Active`
- `Idle`
- `Orphaned`

The view includes summary counts and refreshes while open.

### 7. Dependency Graph

Tree-style dependency view for beads issues. It combines issue list data with
dependency tree data and lets you expand or collapse nodes with `Enter`.

## Agent Lifecycle

1. Obelisk polls beads for ready work.
2. You spawn an agent manually or let auto-spawn pick work.
3. Obelisk launches the selected runtime in a real PTY with the resolved prompt
   template.
4. The agent workflow handles claiming the issue, creating a worktree,
   implementing, verifying, merging, and closing.
5. Obelisk tracks phase progress from PTY output:

```text
Claiming -> Worktree -> Implementing -> Verifying -> Merging -> Closing -> Done
```

6. If the run fails, you can retry with `r`.
7. If the run finishes, you can dismiss it from the dashboard with `x` or `X`.

Important detail: obelisk does not create the git worktree itself. The built-in
agent templates instruct the agent to do that in an isolated `../worktree-<id>`
directory.

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `1`-`7` | Switch views |
| `?` | Toggle help overlay |
| `Esc` | Return to Dashboard from any non-Dashboard view |
| `Ctrl+C` | Force quit |
| `p` | Trigger a manual poll |
| `n` | Toggle desktop notifications |
| `+` / `-` | Increase or decrease max concurrent agents |

### Dashboard

| Key | Action |
|-----|--------|
| `s` | Spawn an agent for the selected ready issue |
| `/` | Open jump-to-issue search |
| `r` | Cycle runtime |
| `m` | Cycle model for the selected runtime |
| `a` | Toggle auto-spawn |
| `c` | Scan and clean orphaned worktrees |
| `w` | Open Worktree Overview |
| `Tab` | Cycle focus: Ready Queue -> Blocked Queue -> Agent List |
| `f` | Cycle ready-queue sort or agent-list status filter, depending on focus |
| `F` | Cycle ready-queue type filter |
| `Enter` | Open Agent Detail when focus is on the Agent List |
| `x` / `X` | Dismiss one or all finished agents from the Agent List |
| `y` | Copy the selected issue ID or agent info |

### Agent Detail

| Key | Action |
|-----|--------|
| `i` | Attach interactive PTY mode |
| `F2` | Detach from interactive PTY mode |
| `d` | Toggle live diff panel |
| `J` / `K` | Scroll the diff panel |
| `e` | Export the selected agent log |
| `r` | Retry a failed agent |
| `D` | Mark the running agent complete and clean up its worktree |
| `k` | Kill the running agent and clean up its worktree |
| `/` | Search PTY output |
| `n` / `N` | Next or previous search match |
| `Up` / `Down` | Scroll output one line |
| `PgUp` / `PgDn` | Scroll output by page |
| `Home` / `End` | Jump to top or re-enable auto-follow |
| `Left` / `Right` | Switch to previous or next agent |
| `y` | Copy the selected agent worktree path |

### Other Views

| View | Keys |
|------|------|
| Event Log | `Up` / `Down` to scroll, `f` to cycle filter |
| History | `Up` / `Down` to scroll, `PgUp` / `PgDn` to move by 10 |
| Split Pane | `Tab` to move focus, `Up` / `Down` to scroll, `Enter` for detail, `g` to pin |
| Worktree Overview | `Up` / `Down` or `j` / `k` to navigate, `f` to change sort |
| Dependency Graph | `Up` / `Down` or `j` / `k` to navigate, `Enter` to expand or collapse |

## Runtimes and Models

Obelisk currently supports these runtime/model combinations:

| Runtime | Models | Invocation |
|---------|--------|------------|
| Claude Code | `claude-sonnet-4-6`, `claude-opus-4-6`, `claude-haiku-4-5-20251001` | `claude "<user_prompt>" --model <model> --append-system-prompt "<system_prompt>" --dangerously-skip-permissions` |
| Codex | `gpt-5.4`, `gpt-5.3-codex`, `gpt-5.3-codex-spark` | `codex --dangerously-bypass-approvals-and-sandbox -m <model> "<combined_prompt>"` |
| Copilot | `claude-sonnet-4`, `gpt-5` | `copilot -i "<combined_prompt>" --model <model> --yolo` |

On Windows, the npm-installed runtimes (`codex` and `copilot`) are wrapped
through `cmd /C ...` so their `.cmd` launchers work correctly under ConPTY.

For Codex and Copilot, obelisk combines the user prompt and system prompt into a
single prompt payload before launching the CLI.

## Prompt Templates

Obelisk resolves prompt templates in this order:

1. `.obelisk/templates/<issue-type>.md`
2. the built-in template for that issue type
3. the built-in `task` template for unknown issue types

Built-in templates exist for:

- `bug`
- `feature`
- `task`
- `chore`
- `epic`

Supported template variables:

- `{id}`
- `{title}`
- `{priority}`
- `{description}`

## Worktrees, Logs, and Persistence

- Agent worktrees are expected to live at `../worktree-<issue-id>`.
- Dashboard cleanup (`c`) removes orphaned worktrees and deletes their branches
  best-effort.
- Exported agent logs (`e`) are written to `logs/<task-id>-<timestamp>.log`.
- Raw PTY logs are also persisted under `.obelisk/logs/`.
- Session records are appended to `.beads/obelisk_sessions.jsonl` when the TUI
  or daemon exits.

Each session record includes:

- `session_id`
- `started_at`
- `ended_at`
- `total_completed`
- `total_failed`
- per-agent runtime/model/elapsed/status data

## Desktop Notifications

When notifications are enabled, obelisk sends native notifications and a
terminal bell for:

- new P0/P1 ready issues
- successful agent completion
- agent failure

## Configuration

Obelisk reads `obelisk.toml` from the working directory if it exists. If the
file is absent, these built-in defaults apply:

```toml
[orchestrator]
runtime = "claude"
max_concurrent = 10
auto_spawn = false
poll_interval_secs = 30
velocity_window = 24

[models]
claude = "claude-sonnet-4-6"
codex = "gpt-5.4"
copilot = "claude-sonnet-4"
```

The repository also includes a checked-in [obelisk.toml](obelisk.toml) with
project-specific overrides.

Supported top-level sections:

- `[orchestrator]`
- `[models]`
- `[theme]`

Supported `[theme]` options:

- `preset` with one of: `solarized`, `nord`, `catppuccin`, `gruvbox`
- individual hex-color overrides for `primary`, `accent`, `secondary`,
  `danger`, `info`, `warn`, `dark_bg`, `panel_bg`, `muted`, `bright`, and
  `dim_accent`

Configuration behavior:

- obelisk hot-reloads config file changes while running
- runtime, models, concurrency, auto-spawn, poll interval, velocity window, and
  theme changes are applied from disk
- current settings are written back to `obelisk.toml` on exit

`velocity_window` is still part of the config schema and is validated/persisted,
but the current dashboard no longer renders a separate velocity sparkline.
