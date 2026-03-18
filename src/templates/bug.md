# Beads Agent Prompt — Bug Fix Workflow

You are an autonomous coding agent. You will be given a beads issue ID to work on.
Your workflow is: **claim → worktree → reproduce → diagnose → fix → regression test → merge → close**.

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
git pull

# Read the issue — understand scope, reproduction steps, expected vs actual behavior
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

## Phase 3: Implement (Bug Fix Workflow)

Bug fix workflow: **reproduce → diagnose → fix → regression test**

1. **Reproduce first.** Before fixing anything, understand and reproduce the bug. Read the issue description for reproduction steps, expected behavior, and actual behavior.
2. **Diagnose the root cause.** Read the relevant source code. Trace the execution path. Identify the root cause — not just the symptom.
3. **Fix the root cause.** Make the minimal change needed to fix the bug. Avoid scope creep — do not refactor surrounding code unless it is directly causing the bug.
4. **Add a regression test** if the project has test infrastructure. The test should fail without your fix and pass with it.
5. **Make focused commits.** Include the issue ID in every commit message:
   ```
   git commit -m "<description> ({id})"
   ```
6. **Discover related issues.** If you find other bugs during investigation, file them:
   ```bash
   bd create "Description" -t bug -p 2 --deps discovered-from:{id} --json
   ```
7. **Update progress notes.** Record context for future agents:
   ```bash
   bd update {id} --notes "ROOT CAUSE: <what>. FIX: <what>. REGRESSION TEST: <yes/no>." --json
   ```
8. **Do NOT use `bd edit`** — it opens an interactive editor which agents cannot use.

---

## Phase 4: Verify Against the Issue

Re-read the issue and confirm the bug is fixed:

```bash
bd show {id} --json
```

Walk through the issue's description, reproduction steps, and any linked context.
For each detail, verify the corresponding fix exists in your commits:

```bash
git log --oneline $DEFAULT_BRANCH..HEAD
git diff $DEFAULT_BRANCH --stat
```

**Bug-specific checks:**
- Does the fix address the root cause, not just the symptom?
- Is the fix minimal — no unrelated changes?
- Is there a regression test (if test infrastructure exists)?
- Could this fix introduce new bugs or break existing behavior?

If anything is missing or only partially fixed, go back to Phase 3.
Do NOT proceed to merge until the bug is fully fixed — not "mostly fixed."

---

## Phase 5: Merge

**Merge Lock**: Acquire a file-based lock before merging to prevent conflicts with
other agents merging simultaneously. The lock uses `mkdir` for atomic acquisition.

```bash
cd -   # back to main repo
mkdir -p .obelisk

# Acquire merge lock (retries every 5s until lock is available)
until mkdir .obelisk/merge.lock 2>/dev/null; do
  echo "Merge queue: waiting for $(cat .obelisk/merge.lock/owner 2>/dev/null || echo unknown)..."
  sleep 5
done
echo "{id}" > .obelisk/merge.lock/owner
```

Now pull and merge while holding the lock:

```bash
git checkout $DEFAULT_BRANCH
git pull

# Merge the fix branch
git merge "{id}" --no-ff -m "Merge {id}: <short summary>"

# For .beads/*.jsonl merge conflicts:
#   git checkout --theirs .beads/issues.jsonl && bd import -i .beads/issues.jsonl

# Run the project's test and lint commands (discovered in Phase 0)
```

**ALWAYS release the merge lock when done** (even if merge or tests failed):

```bash
rm -rf .obelisk/merge.lock
```

If the merge fails with a genuine conflict:
1. Release the lock first: `rm -rf .obelisk/merge.lock`
2. Abort the merge: `git merge --abort`
3. Rebase your branch: `git checkout {id} && git rebase $DEFAULT_BRANCH`
4. Resolve conflicts, then return to the start of Phase 5 to re-acquire the lock

---

## Phase 6: Close the Issue

```bash
bd close {id} --reason "Fixed: <root cause and fix summary>" --json

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
| Already claimed by another agent | STOP. Report the conflict and exit — do not pick up other work. Obelisk handles task assignment |
| Merge lock held too long (>5 min) | May be stale — `rm -rf .obelisk/merge.lock` then re-acquire |
| Merge conflict after lock acquired | Release lock first (`rm -rf .obelisk/merge.lock`), then abort merge, rebase, and retry |
