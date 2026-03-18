# Configuration

Obelisk reads its configuration from `obelisk.toml` in the working directory.
All settings have sensible defaults and the file is optional.

> See also: [Architecture](architecture.md) · [Agent Lifecycle](agent-lifecycle.md)

---

## Config File Format

```toml
[orchestrator]
runtime = "claude"              # "claude", "codex", or "copilot"
max_concurrent = 10             # Maximum agents running simultaneously (1-20)
auto_spawn = false              # Automatically spawn agents from ready queue
poll_interval_secs = 30         # Seconds between bd ready polls
velocity_window = 24            # Data points for velocity sparkline

[models]
claude = "claude-opus-4-6"      # Default model for Claude Code runtime
codex = "gpt-5.4"               # Default model for Codex runtime
copilot = "claude-sonnet-4"     # Default model for Copilot runtime

[theme]
preset = "frost"                # "solarized"/"frost", "nord"/"ember", "catppuccin"/"ash", "gruvbox"/"deep"
# primary = "#6EA0D2"           # Individual hex color overrides (optional)
```

---

## Settings Reference

### Runtime Selection

| Setting | Default | Options |
|---------|---------|---------|
| `runtime` | `"claude"` | `"claude"`, `"codex"`, `"copilot"` |

Controls which CLI is used to spawn agents. Can be changed at runtime with `r`.

### Models

Each runtime has a list of available models. The config file sets the default
selection; cycle through options at runtime with `m`.

**Claude Code models:**
- `claude-sonnet-4-6`
- `claude-opus-4-6`
- `claude-haiku-4-5-20251001`

**Codex models:**
- `gpt-5.4`
- `gpt-5.3-codex`
- `gpt-5.3-codex-spark`

**Copilot models:**
- `claude-sonnet-4`
- `gpt-5`

#### Environment Variable Overrides

Model selections can be overridden via environment variables. These take
precedence over `obelisk.toml` values and are useful for CI, scripting, or
quick one-off changes without editing the config file.

| Variable | Overrides |
|----------|-----------|
| `OBELISK_MODEL_CLAUDE` | `[models] claude` |
| `OBELISK_MODEL_CODEX` | `[models] codex` |
| `OBELISK_MODEL_COPILOT` | `[models] copilot` |

The value must be one of the valid model names listed above. Invalid values
are logged as warnings and ignored.

Example:

```bash
OBELISK_MODEL_CLAUDE=claude-sonnet-4-6 obelisk
```

#### Updating Models

When new model versions are released:

1. Update the model lists in `src/types.rs` (`Runtime::models()`)
2. Update defaults in `obelisk.toml`
3. Update the model lists in this document

Users who have customized `[models]` in their `obelisk.toml` will need to
update those values manually, or use environment variable overrides to
select the new models without changing the file.

### Concurrency

| Setting | Default | Range |
|---------|---------|-------|
| `max_concurrent` | `10` | 1–20 |

Maximum number of agents that can run simultaneously. Adjust at runtime with
`+`/`-`. Auto-spawn respects this limit.

### Auto-Spawn

| Setting | Default |
|---------|---------|
| `auto_spawn` | `false` |

When enabled, obelisk automatically spawns agents for tasks in the ready queue
(up to `max_concurrent`). Toggle at runtime with `a`.

### Poll Interval

| Setting | Default |
|---------|---------|
| `poll_interval_secs` | `30` |

Seconds between automatic polls of `bd ready --json`. A countdown gauge is shown
in the status bar. Manual poll with `p`.

### Velocity Window

| Setting | Default |
|---------|---------|
| `velocity_window` | `24` |

Number of historical session data points used for the velocity sparkline on
the dashboard.

---

## Hardcoded Defaults

These settings are not configurable via `obelisk.toml` but are compiled into
the binary:

| Setting | Value | Description |
|---------|-------|-------------|
| `max_retries` | `3` | Maximum retry attempts for a failed agent |
| `retry_context_lines` | `80` | Lines of failed output included in retry prompt |
| `notifications_enabled` | `true` | Desktop notifications (toggle with `n`) |
| `TICK_RATE_MS` | `100` | UI refresh interval in milliseconds |
| PTY read buffer | `4096` bytes | Chunk size for PTY reader task |
| Event log capacity | `500` entries | Maximum event log entries before oldest are dropped |
| Agent output cap | `10,000` lines | Maximum output lines retained per agent |
| vt100 scrollback | `10,000` lines | Parser scrollback buffer |

---

## Runtime Configuration Persistence

The following settings are saved back to `obelisk.toml` on exit:

- Runtime selection (`runtime`)
- Model selections (`[models]` section)
- Max concurrent (`max_concurrent`)
- Auto-spawn (`auto_spawn`)
- Poll interval (`poll_interval_secs`)
- Velocity window (`velocity_window`)
- Theme configuration (`[theme]` section)

The following are session-only and reset on restart:

- Filter and sort preferences
- Notification toggle

---

## Session Persistence

Session records are appended to `.beads/obelisk_sessions.jsonl` on exit. Each
record contains:

```json
{
  "session_id": "uuid",
  "started_at": "2026-03-12T10:00:00Z",
  "ended_at": "2026-03-12T12:30:00Z",
  "total_completed": 5,
  "total_failed": 1,
  "agents": [...]
}
```

These records populate the History view and the velocity sparkline.

---

## Agent Prompt Templates

Templates control the system prompt sent to agents. Obelisk resolves templates
in this order:

1. `.obelisk/templates/{issue_type}.md` (hot-reloadable, checked each spawn)
2. Built-in template for the issue type (embedded in binary)
3. Built-in `task.md` template (fallback for unknown types)

Available built-in types: `bug`, `feature`, `task`, `chore`, `epic`.

Templates support variable interpolation:

| Variable | Replaced With |
|----------|--------------|
| `{id}` | Issue ID (e.g., `obelisk-xgf`) |
| `{title}` | Issue title |
| `{priority}` | Priority level (integer) |
| `{description}` | Full issue description |

