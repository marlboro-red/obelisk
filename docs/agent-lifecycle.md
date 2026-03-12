# Agent Lifecycle

This document covers the full lifecycle of an agent in obelisk — from spawn
through completion — including PTY mechanics, worktree management, retry logic,
and interactive mode.

> See also: [Architecture](architecture.md) · [Beads Integration](beads-integration.md) · [Troubleshooting](troubleshooting.md)

---

## State Machine

An agent moves through four states:

```
  ┌──────────┐
  │ Starting │  PTY spawned, awaiting first output
  └────┬─────┘
       │ first AgentPtyData received
       ▼
  ┌──────────┐
  │ Running  │  Actively executing, producing output
  └────┬─────┘
       │
  ┌────┴────┐
  │         │
  ▼         ▼
┌───────────┐  ┌────────┐
│ Completed │  │ Failed │
│ (exit 0)  │  │(exit≠0)│
└───────────┘  └────────┘
```

- **Starting → Running**: Triggered when the first PTY data chunk arrives
- **Running → Completed**: Exit code 0
- **Running → Failed**: Non-zero exit code, or manually killed

The exit watcher never overwrites a terminal state — if a killed process fires an
exit event after the status was already set to Failed, the event is ignored.

---

## Phase Detection

Within the Running state, obelisk tracks which workflow phase the agent is in by
scanning PTY output for known command patterns. Phases only advance — they never
retreat.

```
Detecting → Claiming → Worktree → Implementing → Verifying → Merging → Closing → Done
```

| Phase | Trigger Pattern | What It Means |
|-------|----------------|---------------|
| Detecting | (initial) | Agent started, no workflow commands seen yet |
| Claiming | `--claim` | Agent is claiming the beads issue |
| Worktree | `git worktree add` | Creating an isolated worktree |
| Implementing | `--notes` | Writing code, making changes |
| Verifying | `cargo test`, `cargo check`, `cargo clippy` | Running tests/checks |
| Merging | `--no-ff` | Merging feature branch back to main |
| Closing | `bd close` | Closing the beads issue |
| Done | (terminal) | All phases complete |

Phase is displayed in the Agent Detail view as a progress bar.

---

## PTY Spawning

### Process

1. User presses `s` (or auto-spawn triggers) on a ready-queue task
2. `App::get_spawn_info()` builds a `SpawnRequest` with:
   - Task metadata (ID, title, description)
   - Runtime + model selection
   - Interpolated system prompt from templates
3. `spawn_agent_process()` (in main.rs) runs in a tokio task:
   a. Calls `runtime::build_pty_command()` to construct the CLI command
   b. Calls `runtime::spawn_agent_pty()` which uses `portable_pty` to create a PTY pair
   c. Sends `AgentPtyReady` with the master handle back to main thread
   d. Spawns two companion tasks:
      - **PTY reader** (`spawn_blocking`): reads 4KB chunks, sends `AgentPtyData`
      - **Exit watcher** (`spawn_blocking`): calls `child.wait()`, sends `AgentExited`

### PTY Architecture

```
┌─────────────────────────────────────────┐
│ portable_pty                            │
│                                         │
│  Master (parent side)     Slave (child) │
│  ┌───────────────┐   ┌──────────────┐  │
│  │ master fd     │◄─►│ agent CLI    │  │
│  │ + writer      │   │ (claude/     │  │
│  │ + reader      │   │  codex/      │  │
│  └───────┬───────┘   │  copilot)    │  │
│          │           └──────────────┘  │
└──────────┼──────────────────────────────┘
           │
           ▼
    ┌──────────────┐
    │ vt100::Parser│  Terminal emulator — maintains virtual screen
    └──────────────┘
           │
           ▼
    ┌──────────────┐
    │ ratatui      │  Renders screen cells with ANSI colors
    │ Paragraph    │
    └──────────────┘
```

### ConPTY on Windows

On Windows, `portable_pty` uses ConPTY. A critical quirk: ConPTY sends an initial
`ESC[6n` (Device Status Report) query. If unanswered, ConPTY buffers all child
output indefinitely.

Obelisk responds with `ESC[1;1R` (cursor at row 1, col 1) immediately when the
PTY handle is ready. This unblocks the output stream.

### vt100 Parsing

Raw PTY bytes are fed into a `vt100::Parser` (with a 10,000-line scrollback
buffer). The parser processes ANSI escape sequences and maintains a virtual
screen that tracks:
- Cell contents (characters)
- Foreground/background colors (indexed, RGB, default)
- Text attributes (bold, italic, underline, inverse)
- Cursor position

At render time, obelisk reads the parser's screen and maps each cell to a ratatui
`Span` with the appropriate style.

---

## Worktree Creation and Cleanup

### Creation

Obelisk does not create worktrees itself — the agent's system prompt instructs it
to run `git worktree add`. The worktree path follows the convention:

```
../worktree-{issue-id}
```

Obelisk detects the worktree by scanning `git worktree list --porcelain` output
and matching the `worktree-*` naming pattern.

### Tracking

Each `AgentInstance` has:
- `worktree_path: Option<String>` — set when the worktree is detected
- `worktree_cleaned: bool` — set after cleanup

### Cleanup

Cleanup happens in two situations:

1. **On agent kill** (`k` key): After sending SIGTERM/taskkill, obelisk
   runs `git worktree remove <path> --force` followed by a best-effort
   `git branch -D <branch>`.

2. **Manual scan** (`c` key on Dashboard): Scans all `worktree-*` directories,
   excludes those linked to active agents, and cleans the rest.

### Orphan Detection

On startup, obelisk scans for worktrees that have no matching agent instance.
These are marked as "Orphaned" in the Worktree Overview and an alert is shown
prompting the user to clean up (press `c`).

---

## Retry Logic

When an agent fails, it can be retried up to `max_retries` times (default: 3).

### Flow

1. Press `r` in Agent Detail view on a failed agent
2. If `retry_count < max_retries`, obelisk:
   a. Extracts the last `retry_context_lines` (default: 80) lines of PTY output
   b. Runs error pattern detection on those lines
   c. Creates a new `AgentInstance` with:
      - Same task
      - Incremented `retry_count`
      - Augmented user prompt containing failure context

### Error Pattern Detection

The following patterns are detected in failed output:

| Pattern | Detection |
|---------|-----------|
| Rust compilation errors | `error[E` or `error` + `-->` |
| Test failures | `test result: failed`, `failures:` |
| Panics | `thread '...panicked`, `stack backtrace` |
| Permission errors | `permission denied`, `access denied` |
| Git merge conflicts | `merge conflict`, `unmerged paths` |

### Retry Prompt

The retry agent receives the original prompt plus:

```
RETRY CONTEXT (Attempt #N — previous attempt failed)

PREVIOUS ATTEMPT FAILED. Review the failure context below and avoid
repeating the same mistakes. Adjust your approach based on what went wrong.

Exit code: 1
Detected issues:
  - Compilation errors found in output
  - Test failures detected

Last 80 lines of output:
...
```

---

## Interactive Mode

Interactive mode lets you type directly into an agent's PTY, enabling real-time
interaction with the underlying CLI (e.g., answering Claude's questions).

### Attach

Press `i` in Agent Detail view while the agent is Running. All keyboard input
is forwarded to the PTY — normal obelisk keybindings are bypassed.

### Detach

Press `Ctrl+]` (the classic telnet escape sequence). Also auto-detaches when
the agent exits.

### Key Translation

Crossterm key events are translated to terminal escape sequences:

| Key | Bytes Sent |
|-----|-----------|
| `Ctrl+A` through `Ctrl+Z` | `0x01` through `0x1A` |
| `Enter` | `\r` (0x0D) |
| `Backspace` | DEL (0x7F) |
| `Tab` | `\t` |
| `Esc` | `\x1b` |
| Arrow keys | `\x1b[A/B/C/D` |
| `Home` / `End` | `\x1b[H` / `\x1b[F` |
| `PgUp` / `PgDn` | `\x1b[5~` / `\x1b[6~` |
| `Delete` / `Insert` | `\x1b[3~` / `\x1b[2~` |
| `F1`–`F12` | Standard VT100 sequences |
| Printable characters | UTF-8 bytes |

The agent's CLI echoes back through the PTY — obelisk does not add local echo.

---

## Runtime Invocation

### Claude Code

```bash
claude "<user_prompt>" \
  --model <model> \
  --append-system-prompt "<system_prompt>" \
  --dangerously-skip-permissions
```

No `--print` flag — Claude stays interactive after its first response, allowing
the agent to iterate within a single session. This is the only runtime that
supports interactive mode meaningfully.

Available models: `claude-sonnet-4-6`, `claude-opus-4-6`, `claude-haiku-4-5-20251001`

### Codex

```bash
cmd /C codex exec \
  --dangerously-bypass-approvals-and-sandbox \
  -m <model> \
  "<combined_prompt>"
```

One-shot execution. The user and system prompts are combined:
`user_prompt\n\nFollow the workflow below exactly.\n\n---\n\nsystem_prompt`

`cmd /C` wrapping is required on Windows because npm-installed CLIs are `.cmd`
scripts that `CreateProcessW` cannot resolve directly.

Available models: `gpt-5.4`, `gpt-5.3-codex`, `gpt-5.3-codex-spark`

### Copilot

```bash
cmd /C copilot \
  -p "<combined_prompt>" \
  --model <model> \
  --yolo
```

Same combined prompt structure as Codex. `--yolo` bypasses confirmation prompts.

Available models: `claude-sonnet-4`, `gpt-5`

---

## Token Parsing and Cost Tracking

Obelisk parses token counts from CLI output using regex patterns (matched against
each PTY data chunk). Token counts are used to estimate cost based on per-model
pricing:

| Model | Input (per 1M tokens) | Output (per 1M tokens) |
|-------|----------------------|------------------------|
| claude-sonnet-4-6 | $3.00 | $15.00 |
| claude-opus-4-6 | $15.00 | $75.00 |
| claude-haiku-4-5-20251001 | $0.80 | $4.00 |
| gpt-5.4 | $10.00 | $30.00 |

A cost threshold alert (default: $5.00) fires when an agent's estimated cost
exceeds the limit.

---

## Agent Dismissal

Finished agents (Completed or Failed) can be dismissed from the UI:

- `x` — dismiss the selected agent
- `X` — dismiss all finished agents

Dismissal removes the agent from the list and cleans up its PTY state. It does
not affect worktrees (use `c` for worktree cleanup).
