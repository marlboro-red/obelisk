# Architecture

Obelisk is an event-driven Rust TUI that orchestrates autonomous AI agents working on
[Beads](beads-integration.md) issues. It uses **crossterm** for terminal I/O,
**ratatui** for rendering, **portable-pty** for process spawning, and **vt100** for
terminal emulation.

> See also: [Agent Lifecycle](agent-lifecycle.md) · [Beads Integration](beads-integration.md) · [Configuration](configuration.md)

---

## Module Breakdown

```
src/
├── main.rs        Event loop, terminal setup, PTY spawning, keyboard/mouse dispatch
├── app.rs         Core application state (App struct), business logic, event handlers
├── ui.rs          All ratatui rendering — layouts, widgets, color palette, PTY screen
├── runtime.rs     External command construction, PTY creation, polling, worktree ops
├── types.rs       Enums, structs, AppEvent variants, data models
├── templates.rs   Agent prompt templates with variable interpolation
├── theme.rs       Color theming — presets (obsidian, frost, ember, ash, deep), hex overrides
├── daemon.rs      Headless daemon mode — TCP command server, agent lifecycle without TUI
├── client.rs      CLI client for sending commands to a running daemon
└── notify.rs      Desktop notifications and terminal bell
```

### main.rs

Entry point. Sets up the terminal (raw mode, alternate screen, mouse capture),
spawns background tasks, and runs the main event loop.

Key responsibilities:
- Receive events from an unbounded MPSC channel and dispatch them
- Batch PTY data events between UI frames for efficient rendering
- Route keyboard input through mode-aware handlers (normal / interactive / search / jump)
- Spawn agent processes and their companion reader + exit-watcher tasks

### app.rs

Holds the entire `App` struct — the single source of truth for application state.
All mutations flow through event handler methods on `App`.

Key responsibilities:
- Agent lifecycle management (spawn, track, retry, kill, dismiss)
- Poll result processing and ready-queue filtering
- Configuration loading/saving (`obelisk.toml`) with hot-reload
- Session persistence (`.beads/obelisk_sessions.jsonl`)
- Phase detection from PTY output

### ui.rs

Pure rendering logic. Receives `&mut App` (mutable only for ratatui's stateful
list widgets) and draws the current frame.

Key responsibilities:
- 7 view renderers (Dashboard, AgentDetail, EventLog, History, SplitPane, WorktreeOverview, DepGraph)
- Tab bar with badge counts, title bar with animated indicator
- PTY screen rendering (cell-by-cell from vt100 parser)
- Overlays: help, kill confirmation, mark-complete confirmation, search bar, alerts
- Compact layout detection (height < 40 or width < 100)

### runtime.rs

Bridges obelisk to external tools. Constructs CLI commands, manages PTY handles,
and runs git/beads operations.

Key responsibilities:
- Build `CommandBuilder` for each runtime (Claude, Codex, Copilot)
- Spawn PTY via `portable_pty` and return (master, reader, child)
- Poll `bd ready --json`, `bd list -s blocked --json`, and `bd list --json`
- Scan, enrich, and clean up git worktrees
- Compute diffs for the live diff panel

### types.rs

All shared type definitions:
- `AppEvent` — the event enum for cross-task communication
- `AgentInstance`, `BeadTask`, `PtyHandle` — core data models
- `View`, `Runtime`, `AgentStatus`, `AgentPhase` — state enums
- `Focus` — `ReadyQueue`, `BlockedQueue`, `AgentList`
- `DiffData`, `WorktreeEntry`, `DepNode` — view-specific models

### templates.rs

Resolves and interpolates agent prompt templates. Checks `.obelisk/templates/{type}.md`
first, then falls back to built-in templates embedded in the binary.

Variables: `{id}`, `{title}`, `{priority}`, `{description}`.

### theme.rs

Color theming system with preset palettes and per-color hex overrides.

Key responsibilities:
- Default theme ("Obsidian") plus 4 presets (solarized/frost, nord/ember, catppuccin/ash, gruvbox/deep)
- `ThemeConfig` TOML struct for serialization
- `Theme::from_config()` applies preset base + individual color overrides
- Hex color parsing (#RRGGBB or RRGGBB)

### daemon.rs

Headless daemon mode. Runs the orchestrator without the TUI, listening on a TCP
socket (`127.0.0.1`, random port stored in `.beads/obelisk.port`) for JSON
commands from CLI clients.

Key responsibilities:
- Accept JSON commands: `status`, `agents`, `spawn`, `kill`, `stop`
- Manage agent spawning, polling, and lifecycle identically to TUI mode
- Log to stdout/file instead of rendering

### client.rs

CLI client that sends commands to a running daemon. Reads the daemon port from
`.beads/obelisk.port` and communicates via TCP JSON.

### notify.rs

Two functions:
- `send_notification(title, body)` — desktop toast via `notify-rust` (silent fail on unsupported systems)
- `send_bell()` — writes `\x07` to stderr for terminal beep

---

## Event Loop and Message Flow

All cross-thread communication flows through a single `mpsc::UnboundedChannel<AppEvent>`:

```
┌──────────────────┐
│ Crossterm Reader │──→ AppEvent::Terminal(KeyEvent/MouseEvent)
│ (spawn_blocking) │
└──────────────────┘
┌──────────────────┐
│ Tick Timer       │──→ AppEvent::Tick  (every 100ms)
│ (tokio::spawn)   │
└──────────────────┘
┌──────────────────┐
│ Poller           │──→ AppEvent::PollResult / PollFailed  (every 30s)
│ (tokio::spawn)   │
└──────────────────┘
┌──────────────────┐
│ PTY Reader ×N    │──→ AppEvent::AgentPtyData { agent_id, data }
│ (spawn_blocking) │
└──────────────────┘
┌──────────────────┐
│ Exit Watcher ×N  │──→ AppEvent::AgentExited { agent_id, exit_code }
│ (spawn_blocking) │
└──────────────────┘
┌──────────────────┐
│ Worktree Scanner │──→ AppEvent::WorktreeScanned / WorktreeOrphans
│ (tokio::spawn)   │
└──────────────────┘
┌──────────────────┐
│ Diff Poller      │──→ AppEvent::DiffResult { agent_id, diff }
│ (tokio::spawn)   │
└──────────────────┘
┌──────────────────┐
│ DepGraph Poller  │──→ AppEvent::DepGraphResult / DepGraphFailed
│ (tokio::spawn)   │
└──────────────────┘
         │
         ▼
    ┌─────────┐
    │  rx      │  Main event loop (run_app)
    │  recv()  │  ─── wait for first event
    │  drain() │  ─── drain remaining (non-blocking)
    │          │  ─── process_event() for each
    │          │  ─── render() if tick or input
    └─────────┘
```

The loop batches events between frames: PTY data events accumulate while the main
thread processes the previous frame, then all are handled before the next render.
This keeps the UI responsive at ~10 FPS while handling high-throughput PTY streams.

---

## State Management

### The App Struct

`App` is the single mutable state container. Key sections:

```
App
├── Agent & Task State
│   ├── ready_tasks: Vec<BeadTask>         Ready queue from bd
│   ├── agents: Vec<AgentInstance>          All spawned agents
│   ├── claimed_task_ids: HashSet<String>   Prevents double-claim
│   └── pty_states: HashMap<usize, PtyHandle>
│
├── View & Navigation
│   ├── active_view: View                  Current screen (1-7)
│   ├── focus: Focus                       ReadyQueue, BlockedQueue, or AgentList
│   ├── task_list_state / agent_list_state  Cursor positions
│   └── layout_areas: LayoutAreas          For mouse hit-testing
│
├── Filters & Sorting
│   ├── sort_mode: SortMode               Priority/Type/Age/Name
│   ├── type_filter: HashSet<String>
│   └── agent_status_filter: AgentStatusFilter
│
├── Interactive / Search / Jump
│   ├── interactive_mode: bool             PTY input forwarding
│   ├── search_active + search_query       Agent output search
│   └── jump_active + jump_query           Issue ID jump bar
│
├── Configuration
│   ├── selected_runtime: Runtime
│   ├── model_indices: HashMap<Runtime, usize>
│   ├── auto_spawn: bool
│   ├── max_concurrent: usize
│   └── poll_interval_secs: u64
│
└── Metrics & Session
    ├── total_completed / total_failed
    ├── throughput_history: VecDeque<u16>
    ├── session_id, session_started_at
    └── history_sessions: Vec<SessionRecord>
```

### How Views Interact with State

Views are purely visual projections over `App`. Switching views (`active_view`)
changes what `render()` draws and how `handle_key()` routes input, but the
underlying data is always live.

Filters (`sort_mode`, `type_filter`, `agent_status_filter`) are applied at render
time via `filtered_tasks()` and `filtered_agents()` — they never modify the
underlying collections.

---

## Async Task Architecture

| Task | Type | Lifetime | Purpose |
|------|------|----------|---------|
| Terminal reader | `spawn_blocking` | App lifetime | Poll crossterm events (50ms) |
| Tick timer | `tokio::spawn` | App lifetime | 100ms heartbeat |
| Poller | `tokio::spawn` | App lifetime | `bd ready --json` every 30s |
| PTY reader | `spawn_blocking` | Per agent | Read PTY output in 4KB chunks |
| Exit watcher | `spawn_blocking` | Per agent | `child.wait()` → exit code |
| Worktree scan | `tokio::spawn` | On demand | `git worktree list --porcelain` |
| Diff poller | `tokio::spawn` | Periodic (3s) | `git diff` when panel visible |
| DepGraph poller | `tokio::spawn` | Periodic (5s) | `bd list/dep` when view active |

Blocking tasks (`spawn_blocking`) are used for operations that block the thread:
reading from PTY file descriptors and waiting for child process exit. Async tasks
are used for operations that can yield (network I/O, shell command output).

---

## Rendering Pipeline

```
render(f, app)
  │
  ├── Clear screen + dark background
  ├── Detect compact layout (height < 40 or width < 100)
  │
  ├── Vertical layout split:
  │   ├── Title bar           (3 rows)
  │   ├── Tab bar + badges    (1 row)
  │   ├── Main content        (flexible)
  │   ├── Status gauges       (3 rows, hidden in compact)
  │   ├── Info bar            (3 rows, hidden in compact)
  │   └── Keybindings footer  (1 row)
  │
  ├── View-specific renderer:
  │   ├── Dashboard:  ready queue + blocked queue | agent list (horizontal split)
  │   ├── AgentDetail: header + PTY screen + optional diff panel
  │   ├── EventLog:   filtered log entries
  │   ├── History:    session records table
  │   ├── SplitPane:  2×2 grid of agent outputs
  │   ├── WorktreeOverview: worktree table with status badges
  │   └── DepGraph:   tree with expand/collapse
  │
  └── Overlays (if active):
      ├── Help overlay (?)
      ├── Kill confirmation dialog
      ├── Jump bar (/)
      ├── Search bar (/)
      └── Alert banner
```

### PTY Screen Rendering

Agent output is not stored as plain text lines. Instead, raw PTY bytes are fed
into a `vt100::Parser` which maintains a virtual terminal screen. At render time:

1. Access the parser's `screen()` — a 2D grid of cells
2. Iterate rows × cols within the visible area
3. Extract each cell's character + foreground/background/attributes
4. Map vt100 colors to ratatui `Color` values
5. Apply search highlights over matching positions
6. Emit as a ratatui `Paragraph` widget

This preserves ANSI colors, cursor positioning, and progress bars exactly as the
agent's CLI renders them.

### Terminal Resize Handling

On resize:
1. `terminal.clear()` forces a full repaint (prevents stale cell artifacts)
2. `sync_pty_sizes()` resizes every active PTY's master fd
3. Each PTY's vt100 parser is replaced with a fresh instance at the new dimensions
   (avoids garbled reflow artifacts from `set_size()`)

---

## Color Palette

The default theme is "Obsidian" — a subdued, monochromatic palette with restrained
accents. See `theme.rs` for all presets (frost, ember, ash, deep) and per-color
hex overrides via `obelisk.toml`.

```
PRIMARY:    rgb(110, 160, 210)  Steel blue   Borders, focus, titles
ACCENT:     rgb(110, 175, 140)  Sage         Success, completion
SECONDARY:  rgb(130, 130, 150)  Blue-gray    Subdued IDs, metadata
DANGER:     rgb(190, 80, 75)    Muted rust   Errors, failures
INFO:       rgb(110, 160, 210)  = primary    Informational
WARN:       rgb(195, 165, 90)   Muted gold   Caution, cost
DARK_BG:    rgb(14, 14, 20)     Near-black   Application background
PANEL_BG:   rgb(20, 20, 28)     Dark gray    Panel backgrounds
MUTED:      rgb(60, 60, 75)     Dim gray     Labels, chrome
BRIGHT:     rgb(185, 185, 200)  Silver       Primary text
DIM_ACCENT: rgb(60, 95, 75)     Dimmed sage  Subtle accents
```

---

## Key Architectural Patterns

**Event-driven single-channel** — All external data flows through one `AppEvent`
enum and one MPSC channel. This simplifies synchronization and makes the data flow
easy to trace.

**Frame-based batching** — Events accumulate between renders. The loop drains all
pending events before each frame, keeping the UI responsive under high PTY
throughput.

**Filter-at-render** — Filters create temporary views over collections without
copying or mutating. Underlying data stays intact.

**PTY terminal emulation** — Real terminal emulation via vt100 preserves CLI
rendering fidelity (colors, progress bars, cursor positioning) rather than
treating output as plain text.

**Graceful degradation** — Poll failures increment a counter and show a banner
but don't crash. Missing templates fall back to built-ins. PTY spawn failures
mark the agent as Failed. Missing config uses hardcoded defaults.

**Platform abstraction** — `cfg(windows)` / `cfg(not(windows))` blocks handle
npm CLI wrapping (`cmd /C`) and signal delivery (`taskkill` vs `SIGTERM`).
`portable_pty` abstracts ConPTY vs Unix PTY.
