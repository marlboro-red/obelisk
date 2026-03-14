# Keybindings

Complete keybinding reference for obelisk, organized by view. Keys marked
**Global** work in all views. Context-dependent keys are noted where behavior
changes based on focus or state.

> See also: [Architecture](architecture.md) · [Agent Lifecycle](agent-lifecycle.md)

---

## Global

| Key | Action |
|-----|--------|
| `?` | Toggle help overlay |
| `Ctrl+C` | Force quit application |
| `1`–`7` | Switch to view (Dashboard, Agent Detail, Event Log, History, Split Pane, Worktree Overview, Dep Graph) |
| `Esc` | Return to Dashboard (from any non-Dashboard view) |
| `q` | Quit application (from Dashboard) or return to Dashboard (from other views) |
| `Tab` | Cycle focus between panels (Dashboard: Ready Queue → Blocked Queue → Agent List) |
| `n` | Toggle desktop notifications on/off |
| `p` | Trigger manual poll / scan |
| `+` or `=` | Increase max concurrent agent slots |
| `-` | Decrease max concurrent agent slots |
| `y` | Copy selected item info to clipboard |

---

## Dashboard

### Ready Queue Panel (focus: Ready Queue)

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `s` | Spawn agent on selected task |
| `Enter` | Spawn agent on selected task |
| `/` | Open jump-to-issue bar |
| `r` | Cycle runtime (Claude → Codex → Copilot) |
| `m` | Cycle model for current runtime |
| `a` | Toggle auto-spawn mode |
| `f` | Cycle sort mode (Priority → Type → Age → Name) |
| `F` | Cycle type filter (bug → feature → task → chore → epic → all) |
| `c` | Scan and clean orphaned worktrees |
| `w` | Open Worktree Overview |

### Blocked Queue Panel (focus: Blocked Queue)

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `y` | Copy blocked issue ID to clipboard |

### Agent List Panel (focus: Agent List)

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `Enter` | Open Agent Detail for selected agent |
| `f` | Cycle agent status filter (All → Running → Failed → Done → Init) |
| `x` | Dismiss selected finished agent |
| `X` | Dismiss ALL finished agents |
| `y` | Copy agent info to clipboard |

---

## Agent Detail — Observe Mode

| Key | Action |
|-----|--------|
| `i` | Attach to interactive PTY session |
| `↑` | Scroll output up one line |
| `↓` | Scroll output down one line |
| `PgUp` | Page up in output |
| `PgDn` | Page down in output |
| `Home` | Jump to top of output |
| `End` | Re-engage auto-follow (scroll to bottom) |
| `←` | Switch to previous agent |
| `→` | Switch to next agent |
| `/` | Open search bar |
| `d` | Toggle live git diff panel |
| `J` | Scroll diff panel down (when visible) |
| `K` | Scroll diff panel up (when visible) |
| `e` | Export agent log to file |
| `r` | Retry failed agent |
| `D` | Mark agent as completed + SIGTERM + clean up worktree |
| `k` | Kill agent (shows confirmation) |

### Search Mode (within Agent Detail)

Activated by pressing `/` in observe mode.

| Key | Action |
|-----|--------|
| Any character | Append to search query |
| `Backspace` | Remove last character |
| `n` | Next match |
| `N` | Previous match |
| `Esc` | Close search bar |

---

## Agent Detail — Interactive Mode

Activated by pressing `i` in observe mode. All keys are forwarded to the agent's
PTY as terminal escape sequences.

| Key | Action |
|-----|--------|
| `F2` | Detach (return to observe mode) |
| All other keys | Forwarded to PTY |

PTY key mappings:

| Key | Bytes |
|-----|-------|
| `Ctrl+A`–`Ctrl+Z` | `0x01`–`0x1A` |
| `Enter` | `\r` |
| `Backspace` | `0x7F` (DEL) |
| `Tab` | `\t` |
| `Esc` | `\x1b` |
| `↑` / `↓` / `→` / `←` | `\x1b[A` / `B` / `C` / `D` |
| `Home` / `End` | `\x1b[H` / `\x1b[F` |
| `PgUp` / `PgDn` | `\x1b[5~` / `\x1b[6~` |
| `Delete` / `Insert` | `\x1b[3~` / `\x1b[2~` |
| `F1`–`F12` | Standard VT100 sequences |

---

## Event Log

| Key | Action |
|-----|--------|
| `↑` | Scroll up |
| `↓` | Scroll down |
| `f` | Cycle category filter (All → System → Incoming → Deploy → Complete → Alert → Poll) |

---

## History

| Key | Action |
|-----|--------|
| `↑` | Navigate up |
| `↓` | Navigate down |
| `PgUp` | Scroll up by 10 |
| `PgDn` | Scroll down by 10 |

---

## Split Pane

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus between panes (0 → 1 → 2 → 3 → 0) |
| `↑` | Scroll focused pane output up |
| `↓` | Scroll focused pane output down |
| `Enter` | Open Agent Detail for agent in focused pane |
| `g` | Pin/unpin agent to focused pane slot |

---

## Worktree Overview

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `f` | Cycle sort mode (Age ↔ Status) |

---

## Dependency Graph

| Key | Action |
|-----|--------|
| `↑` / `k` | Navigate up |
| `↓` / `j` | Navigate down |
| `Enter` | Expand/collapse subtree at current node |

---

## Kill Confirmation Dialog

Shown after pressing `k` in Agent Detail.

| Key | Action |
|-----|--------|
| `y` or `Enter` | Confirm kill and clean up worktree |
| `n` or `Esc` | Cancel |

---

## Jump-to-Issue Bar

Shown after pressing `/` on the Dashboard ready queue.

| Key | Action |
|-----|--------|
| Any character | Append to query |
| `Backspace` | Remove last character |
| `Enter` | Jump to matching issue/agent |
| `Esc` | Close jump bar |

---

## Mouse Events

Mouse support is enabled by default.

| Event | Area | Action |
|-------|------|--------|
| Left click | Tab bar | Switch to corresponding view |
| Left click | Ready queue | Select task at clicked row |
| Left click | Agent list | Select agent at clicked row |
| Left click | Split pane | Focus clicked pane |
| Scroll up/down | Ready queue / Agent list | Navigate selection |
| Scroll up/down | Agent Detail output | Scroll by 3 lines |
| Scroll up/down | Event Log / History | Scroll |
| Scroll up/down | Split pane | Scroll focused pane |
| Scroll up/down | Worktree / Dep Graph | Navigate |

---

## Context-Dependent Keys

Some keys have different behavior depending on the active view or focus:

| Key | Dashboard (Ready Queue) | Dashboard (Agent List) | Agent Detail | Other Views |
|-----|------------------------|----------------------|--------------|-------------|
| `f` | Cycle sort mode | Cycle status filter | — | Cycle view-specific filter |
| `/` | Jump-to-issue | — | Search PTY output | — |
| `r` | Cycle runtime | — | Retry failed agent | — |
| `k` | Navigate up | — | Kill agent | Navigate up |
| `Enter` | Spawn agent | Agent Detail | — | View-specific action |
