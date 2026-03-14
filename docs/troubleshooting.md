# Troubleshooting

Common issues and their solutions when running obelisk.

> See also: [Architecture](architecture.md) · [Agent Lifecycle](agent-lifecycle.md) · [Configuration](configuration.md)

---

## Windows ConPTY Quirks

### Agent output doesn't appear

**Symptom:** Agent spawns (status shows Starting) but no output appears.

**Cause:** ConPTY sends an initial `ESC[6n` Device Status Report query. If
unanswered, it buffers all child output.

**Fix:** Obelisk automatically responds with `ESC[1;1R` when the PTY handle is
ready. If output still doesn't appear:
- Ensure you're on Windows 10 build 17763+ (ConPTY support)
- Try running in Windows Terminal rather than legacy cmd.exe
- Check that `portable-pty` is at version 0.9+

### Resize artifacts

**Symptom:** Visual glitches or garbled text after resizing the terminal window.

**Cause:** ConPTY reflow can produce intermediate states between resize and the
child process's SIGWINCH-triggered repaint.

**Fix:** Obelisk mitigates this by:
1. Clearing the entire screen on resize (`terminal.clear()`)
2. Replacing the vt100 parser with a fresh instance at the new dimensions
   (avoids garbled reflow from `set_size()`)

If artifacts persist, press any key to trigger a repaint, or switch views
and back.

### npm CLIs fail to launch

**Symptom:** Codex or Copilot agents fail immediately with "not found" or similar.

**Cause:** On Windows, npm-installed CLIs are `.cmd` scripts. `CreateProcessW`
cannot resolve these directly — they must be launched through `cmd /C`.

**Fix:** Obelisk handles this automatically with platform-specific command
wrapping (`cmd /C codex ...` on Windows vs `codex ...` on Unix). If you still
see errors:
- Verify the CLI is installed: `codex --version` or `copilot --version`
- Ensure the npm global bin directory is in your PATH
- Try running the command manually: `cmd /C codex --help`

---

## Unix-Specific Issues

### Signal delivery

On Unix, obelisk sends `SIGTERM` via `libc::kill(pid, SIGTERM)` when killing an
agent. On Windows, it uses `taskkill /PID <pid> /T /F` for tree kill.

If an agent doesn't respond to kill:
- The PTY reader will eventually detect EOF and trigger the exit event
- As a last resort, manually kill the process: `kill -9 <pid>`

### PTY permissions

**Symptom:** Agent fails immediately with "permission denied".

**Fix:** Ensure the agent CLI binary has execute permissions:
```bash
chmod +x $(which claude)
```

---

## Poll Failures

### "Check dolt server status" alert

**Symptom:** Alert banner appears after 3+ consecutive poll failures.

**Cause:** `bd ready --json` is failing repeatedly. Common reasons:
- The beads database (`.beads/`) is missing or corrupt
- `bd` CLI is not installed or not in PATH
- Git repository is in an inconsistent state

**Diagnosis:**
```bash
# Verify bd is available
bd --version

# Test the poll command manually
bd ready --json

# Check the beads database
ls -la .beads/
```

### Poll succeeds but ready queue is empty

**Cause:** All issues may be claimed, in progress, or closed. Check:
```bash
bd list --json
```

---

## Agent Stuck in Starting State

**Symptom:** Agent shows "Starting" status indefinitely, no output appears.

**Possible causes:**

1. **ConPTY DSR not answered** (Windows) — see "Agent output doesn't appear" above
2. **CLI not found** — the agent runtime binary isn't in PATH
3. **API credentials missing** — the agent CLI can't authenticate
4. **PTY spawn failure** — check the event log for error messages

**Diagnosis:**
- Check the Event Log view (`3`) for deployment errors
- Try running the agent command manually in a terminal
- Verify API credentials are set (environment variables or config files)

**Recovery:**
- Press `k` to kill the stuck agent
- Fix the underlying issue
- Press `r` to retry (or `s` to spawn fresh)

---

## Worktree Issues

### Orphaned worktrees on startup

**Symptom:** Alert on startup about orphaned worktrees.

**Cause:** A previous session exited without cleaning up agent worktrees
(crash, force kill, etc.).

**Fix:** Press `c` on the Dashboard to scan and clean orphaned worktrees. Or
press `6` (Worktree Overview) to see all worktrees and their status.

Manual cleanup:
```bash
git worktree list
git worktree remove ../worktree-<issue-id> --force
git branch -D <issue-id>
```

### Worktree removal fails

**Symptom:** Cleanup reports failures for some worktrees.

**Possible causes:**
- Files are locked by another process (editor, file watcher)
- Insufficient permissions
- The worktree directory was already manually deleted but git still tracks it

**Fix:**
```bash
# Force remove from git's tracking
git worktree remove ../worktree-<id> --force

# If the directory was already deleted
git worktree prune
```

### Branch already exists

**Symptom:** Agent fails during worktree creation with "branch already exists".

**Cause:** A previous agent created the branch but didn't clean up.

**Fix:**
```bash
git branch -D <issue-id>
```

---

## Terminal Rendering

### Minimum terminal size

Obelisk adapts its layout for different terminal sizes:
- **Width < 100**: Compact horizontal layout (no stats sidebar in Agent Detail)
- **Height < 40**: Compact vertical layout (hides status gauges and info bar)
- **Width < 120**: No stats panel in Agent Detail
- **PTY resize ignored if**: rows < 2 or cols < 10

If the terminal is very small, some UI elements may be truncated or hidden.
Recommended minimum: 80 columns × 24 rows.

### Colors look wrong

**Cause:** Terminal doesn't support 24-bit (truecolor) RGB.

**Fix:** Use a terminal that supports truecolor:
- Windows Terminal (recommended on Windows)
- iTerm2 (macOS)
- Most modern Linux terminal emulators

Check truecolor support: `echo -e "\x1b[38;2;255;103;0mOrange\x1b[0m"`

### Flickering or slow rendering

**Cause:** Terminal emulator is slow at processing escape sequences.

**Mitigations:**
- Use a GPU-accelerated terminal (Windows Terminal, Alacritty, Kitty)
- Obelisk renders at ~10 FPS (100ms tick) and batches PTY events to minimize
  redundant frames
- Large PTY output may slow rendering — the vt100 parser uses a 10,000-line
  scrollback buffer

---

## Agent Retries

### "Max retries reached"

**Symptom:** Can't retry a failed agent.

**Cause:** The agent has already been retried `max_retries` times (default: 3).

**Workaround:** Spawn a fresh agent for the same task from the ready queue.
The fresh agent starts with `retry_count = 0`.

### Retry agent makes the same mistake

**Cause:** The retry context (last 80 lines + error patterns) may not capture
the root cause.

**Fix:** Kill the agent, investigate the failure manually, then fix any
environmental issues before spawning again.

---

## Log Export

### Exported log is binary / has escape codes

**By design.** The exported log (`.beads/logs/agent-*.log`) contains raw PTY
bytes including ANSI escape sequences. This preserves the full terminal output
for replay.

To view with colors: `cat .beads/logs/agent-01.log`

To strip escape codes: `sed 's/\x1b\[[0-9;]*m//g' .beads/logs/agent-01.log`

---

## Exit Code Limitations

`portable_pty::ExitStatus` does not expose the raw exit code — only whether the
process succeeded (exit 0) or failed (non-zero). Obelisk reports:
- `Some(0)` for success
- `Some(1)` for any non-zero exit
- `None` if `child.wait()` itself failed

This means you cannot distinguish between exit codes 1, 2, 127, etc. from within
obelisk. Check the agent's PTY output for specific error messages.
