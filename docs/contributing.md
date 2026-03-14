# Contributing

How to build, run, and extend obelisk.

> See also: [Architecture](architecture.md) · [Keybindings](keybindings.md)

---

## Build and Run

### Prerequisites

- **Rust** (2021 edition) — install via [rustup](https://rustup.rs)
- **Git** — for worktree management
- **bd** (Beads CLI) — for task polling
- **Agent CLI** — at least one of: `claude`, `codex`, `copilot`
- **Windows 10 build 17763+** (for ConPTY support on Windows)

### Build

```bash
cargo build --release
```

The binary is at `target/release/obelisk` (or `obelisk.exe` on Windows).

### Run

```bash
# From a directory with a .beads/ database
cargo run --release

# Or run the binary directly
./target/release/obelisk
```

Obelisk expects:
- A `.beads/` directory in the current working directory (or parent)
- `bd` CLI in PATH
- `git` CLI in PATH
- Valid API credentials for the selected runtime (environment variables)

### Configuration

Copy and edit the example config:
```bash
cp obelisk.toml.example obelisk.toml
# Or create from scratch — see docs/configuration.md
```

---

## Project Structure

```
src/
├── main.rs        Entry point, event loop, PTY spawning, input dispatch
├── app.rs         App struct, business logic, all event handlers
├── ui.rs          Rendering — layouts, widgets, color palette
├── runtime.rs     CLI command building, PTY creation, git/beads operations
├── types.rs       Shared types — enums, structs, AppEvent
├── templates.rs   Agent prompt template resolution and interpolation
├── theme.rs       Color theming — presets, hex overrides, ThemeConfig
├── daemon.rs      Headless daemon mode — TCP server, agent lifecycle without TUI
├── client.rs      CLI client for daemon IPC
└── notify.rs      Desktop notifications and terminal bell

docs/              You are here
Cargo.toml         Dependencies and build configuration
obelisk.toml       Runtime configuration (user-edited)
AGENTS.md          Agent instruction templates
```

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui 0.30` | Terminal UI framework (widgets, layouts, styling) |
| `crossterm 0.28` | Cross-platform terminal I/O (raw mode, events, colors) |
| `tokio` (full) | Async runtime (tasks, timers, channels) |
| `portable-pty 0.9` | Cross-platform PTY spawning (ConPTY on Windows) |
| `vt100 0.16` | Terminal emulator / ANSI parser |
| `serde` + `serde_json` | JSON serialization (beads data, session records) |
| `toml 0.8` | Config file parsing |
| `chrono 0.4` | Timestamp handling |
| `libc 0.2` | Unix signal delivery (SIGTERM) |
| `arboard 3` | Clipboard access (copy with `y`) |
| `notify-rust 4` | Desktop notification toasts |
| `regex 1` | Token parsing from CLI output |
| `anyhow 1` | Error handling |

---

## Code Conventions

### Naming

- **Functions/variables:** `snake_case` (Rust standard)
- **Types/enums:** `PascalCase`
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `TICK_RATE_MS`, `PRIMARY`)
- **Agent IDs:** `agent_id: usize` (internal), displayed as `AGENT-01` (unit_number)

### Error Handling

- `Result<T, String>` for most functions (explicit error messages)
- `anyhow::Result` for PTY spawn operations (wraps multiple error types)
- `let _ = ...` for best-effort operations (e.g., branch deletion after worktree cleanup)
- `?` operator for error propagation

### Patterns

- **Saturating arithmetic** throughout (`saturating_sub`, `saturating_add`) to prevent panics on underflow
- **Builder pattern** for CLI commands: `CommandBuilder::new(...).arg(...)`
- **Clone-heavy** for PTY handles (owned by main thread, shared across tasks)
- **`LazyLock<Regex>`** for compile-once regex patterns

---

## How to Add a New View

1. **Add the variant** to `View` enum in `types.rs`:
   ```rust
   pub enum View {
       // ...existing views...
       MyNewView,
   }
   ```

2. **Add the renderer** in `ui.rs`:
   ```rust
   fn render_my_new_view(f: &mut Frame, app: &mut App, area: Rect) {
       // Your rendering logic
   }
   ```

3. **Wire it into the render dispatch** in `ui.rs`:
   ```rust
   match app.active_view {
       // ...existing views...
       View::MyNewView => render_my_new_view(f, app, main_area),
   }
   ```

4. **Assign a number key** in `main.rs` `handle_key()`:
   ```rust
   KeyCode::Char('8') => {
       app.active_view = View::MyNewView;
   }
   ```

5. **Add a tab** in `ui.rs` tab bar rendering (look for the tab labels array).

6. **Add keybindings** for the new view in `handle_key()` if needed.

7. **Update the help overlay** in `ui.rs` to document new keybindings.

---

## How to Add a New Keybinding

1. **Find the right handler** in `main.rs`:
   - Global keys: top of `handle_key()`
   - View-specific: inside the `match app.active_view` block
   - Mode-specific: inside the search/jump/interactive mode blocks

2. **Add the match arm**:
   ```rust
   KeyCode::Char('z') => {
       app.do_something();
   }
   ```

3. **Implement the handler** on `App` in `app.rs` if it involves state changes.

4. **Update the keybindings footer** in `ui.rs` (look for the keybinding line
   at the bottom of each view's renderer).

5. **Update the help overlay** in `ui.rs` (search for the help text block).

6. **Update `docs/keybindings.md`** to document the new binding.

---

## How to Add a New Runtime

1. **Add the variant** to `Runtime` enum in `types.rs`:
   ```rust
   pub enum Runtime {
       // ...existing...
       MyRuntime,
   }
   ```

2. **Add model list** in `types.rs` (add a match arm to `Runtime::models()`
   and `Runtime::display_name()`).

3. **Add command building** in `runtime.rs`:
   ```rust
   Runtime::MyRuntime => {
       let mut cmd = CommandBuilder::new("my-cli");
       cmd.arg("--prompt").arg(&combined);
       cmd.arg("--model").arg(model);
       cmd
   }
   ```

4. **Add token pricing** in `app.rs` (search for the pricing match block).

5. **Add config support** in `app.rs` config loading/saving.

6. **Update the runtime cycle** in `main.rs` (the `r` key handler).

---

## Testing

### Running Tests

```bash
cargo test
```

### Test Coverage

Unit tests are at the bottom of modules with `#[cfg(test)]`. Current test
coverage focuses on:

- **PTY area computation** (`ui.rs`) — tests edge cases at various terminal
  sizes: tiny (20×10), standard (80×24), wide (200×50), and boundary cases
  (119 vs 120 cols, 39 vs 40 rows)
- **App state and config** (`app.rs`) — agent lifecycle transitions, config
  validation (unknown keys, out-of-range values, theme preset aliases, model
  validation), poll result processing, event logging
- **Theme system** (`theme.rs`) — hex color parsing, preset selection, alias
  resolution (frost→solarized, ember→nord, ash→catppuccin, deep→gruvbox),
  config overrides, serde round-trip
- **Notifications** (`notify.rs`) — bell output

### Adding Tests

Add tests in the relevant module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_feature() {
        // ...
    }
}
```

For async tests:
```rust
#[tokio::test]
async fn test_async_feature() {
    // ...
}
```

---

## Debugging

### Event Log

The built-in Event Log view (`3`) shows system events, poll results, agent
deployments, completions, and alerts. This is the first place to check when
diagnosing issues.

### Log Export

Press `e` in Agent Detail to export the full PTY log to `.beads/logs/agent-*.log`.
This captures raw terminal output including ANSI sequences.

### Manual Testing

To test changes without a live beads database:
1. Create a minimal `.beads/` setup
2. Add test issues: `bd create "Test issue" -t task -p 2`
3. Run obelisk and spawn agents manually

### Metrics

The Dashboard shows live metrics:
- **Throughput:** Lines per second (rolling 60-entry window)
- **Velocity:** Completed agents per session (sparkline)
- **Poll health:** Last result status, consecutive failures
- **Per-agent:** Elapsed time, tokens, estimated cost, phase
