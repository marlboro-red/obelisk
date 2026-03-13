# Beads Integration

Obelisk orchestrates work from the [Beads](https://github.com/user/beads) task
management system via its `bd` CLI. This document covers the poll cycle, ready
queue, issue lifecycle, dependency awareness, and the agent prompt template system.

> See also: [Agent Lifecycle](agent-lifecycle.md) · [Configuration](configuration.md) · [Architecture](architecture.md)

---

## Overview

```
┌──────────────┐     bd ready --json      ┌──────────────┐
│              │ ◄───────────────────────  │              │
│  .beads/     │                           │   obelisk    │
│  database    │  bd update --claim        │   TUI        │
│              │ ◄───────────────────────  │              │
│              │                           │              │
│              │  bd close --reason "..."  │              │
│              │ ◄───────────────────────  │              │
└──────────────┘                           └──────────────┘
        ▲                                        │
        │              spawn agent               │
        └────────── (agent runs bd commands) ─────┘
```

Obelisk polls the beads database for ready issues, displays them in the ready
queue, and spawns agents to work on them. The agents themselves run `bd` commands
(claim, update notes, close) — obelisk does not claim or close issues directly.

---

## Poll Cycle

### Automatic Polling

A background tokio task runs `bd ready --json` at a configurable interval
(default: 30 seconds, set via `poll_interval_secs` in `obelisk.toml`).

The poll cycle:

1. Execute `bd ready --json`
2. Parse the JSON array of `BeadTask` objects
3. Filter out tasks whose IDs are in the `claimed_task_ids` set
4. Send `AppEvent::PollResult(tasks)` to the main event loop
5. Sleep for `poll_interval_secs`

On failure, `AppEvent::PollFailed(error)` is sent instead.

### Manual Polling

Press `p` at any time to trigger an immediate poll, resetting the countdown.

### Poll Health

Obelisk tracks consecutive poll failures:
- `last_poll_ok: bool` — whether the last poll succeeded
- `consecutive_poll_failures: u32` — failure streak counter
- At 3+ consecutive failures, an alert banner appears: "check dolt server status"

The poll countdown is displayed as a gauge in the status bar.

---

## Ready Queue Population

When a `PollResult` arrives:

1. New tasks are merged into `ready_tasks`
2. Tasks already in `claimed_task_ids` are excluded
3. New high-priority tasks (P0, P1) trigger:
   - An alert banner in the UI
   - A desktop notification (if enabled)
   - A terminal bell
4. Tasks are sorted according to the current `sort_mode`

### Sort Modes

Cycle through sort modes with `f` (when Ready Queue is focused):

| Mode | Sort Order |
|------|-----------|
| Priority | Lowest number first (P0 > P1 > P2 > ...) |
| Type | Alphabetical by issue type |
| Age | Oldest first (by `created_at`) |
| Name | Alphabetical by title |

### Type Filter

Cycle through type filters with `F`:

`all` → `bug` → `feature` → `task` → `chore` → `epic` → `all`

---

## Issue Claiming and Status Updates

Obelisk does not interact with `bd update` or `bd close` directly. Instead, the
agent's system prompt (from [templates](configuration.md#agent-prompt-templates))
instructs the agent to follow this workflow:

```
1. bd update <id> --claim          # Claim the issue (atomic)
2. git worktree add ...            # Create isolated worktree
3. <implement changes>             # Write code, run tests
4. bd update <id> --notes "..."    # Record progress
5. git merge ...                   # Merge back to main
6. bd close <id> --reason "..."    # Close the issue
```

Obelisk detects these phases by scanning PTY output for command patterns
(see [Phase Detection](agent-lifecycle.md#phase-detection)).

When an agent is spawned, its task ID is added to `claimed_task_ids` to prevent
double-claiming by another agent.

---

## Dependency Awareness

### Dependency Graph View

Press `7` to open the Dependency Graph view. Obelisk polls dependency data
every ~5 seconds while this view is active:

1. `bd list --json` — fetches all issues
2. `bd dep tree <id> --direction both --json` — enriches each issue with
   parent/child relationships

The result is a flat list with `depth` and `parent_id` fields, rendered as an
expandable tree. Press `Enter` to expand/collapse subtrees.

### Blocked Issues

The beads workflow prompt instructs agents to check for blockers before starting
work. If an issue has unresolved `blocked_by` dependencies, the agent should
stop and report back rather than proceeding.

Obelisk itself does not enforce blocking — it relies on the agent's adherence to
the prompt template.

---

## Agent Prompt Template

When spawning an agent, obelisk builds two prompt components:

### User Prompt

The task description from the beads issue, interpolated into the template:

```
Work on beads issue {id}. Follow the workflow in the Beads Agent Prompt exactly.
```

### System Prompt

Resolved from templates (see [Configuration — Templates](configuration.md#agent-prompt-templates)):

1. Check `.obelisk/templates/{issue_type}.md`
2. Fall back to built-in template
3. Interpolate variables: `{id}`, `{title}`, `{priority}`, `{description}`

The system prompt contains the full beads workflow phases, guiding the agent
through claim → worktree → implement → verify → merge → close.

### Runtime-Specific Prompt Handling

| Runtime | Prompt Delivery |
|---------|----------------|
| Claude Code | `--append-system-prompt` flag (separate user + system) |
| Codex | Combined into single prompt: `user\n\nFollow the workflow below exactly.\n\n---\n\nsystem` |
| Copilot | Combined into single prompt: `user\n\nFollow the workflow below exactly.\n\n---\n\nsystem` |

---

## Data Flow Summary

```
bd ready --json
  │
  ▼
PollResult → App.on_poll_result()
  │           ├── Filter claimed_task_ids
  │           ├── Sort by sort_mode
  │           └── Alert on P0/P1
  │
  ▼
Ready Queue (UI)
  │
  ▼  (user presses 's' or auto-spawn)
SpawnRequest → spawn_agent_process()
  │             ├── Resolve template
  │             ├── Build CLI command
  │             ├── Spawn PTY
  │             └── Add to claimed_task_ids
  │
  ▼
Agent runs bd commands autonomously:
  bd update --claim → bd update --notes → bd close --reason
```
