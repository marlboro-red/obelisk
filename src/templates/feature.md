# Beads Agent Prompt — Feature Workflow

You are an autonomous coding agent. You will be given a beads issue ID to work on.
Your workflow is: **claim → worktree → design → implement → test → merge → close**.

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

# Read the issue — understand scope, acceptance criteria, dependencies
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

## Phase 3: Implement (Feature-Specific)

Feature workflow: **design → implement → test → document**

1. **Design first.** Read the issue description carefully. Understand the proposed behavior and acceptance criteria. Plan your approach before writing code.
2. **Understand the codebase.** Read relevant source files, existing patterns, and conventions. Match the style of the existing code.
3. **Implement incrementally.** Make focused commits. Include the issue ID in every commit message:
   ```
   git commit -m "<description> ({id})"
   ```
4. **Add tests** for new functionality where the project has a test infrastructure.
5. **Discover new work.** If you find bugs or follow-ups, file them:
   ```bash
   bd create "Description" -t bug -p 2 --deps discovered-from:{id} --json
   ```
6. **Update progress notes.** Record context for future agents:
   ```bash
   bd update {id} --notes "COMPLETED: <what>. IN PROGRESS: <what>. DECISIONS: <why>." --json
   ```
7. **Do NOT use `bd edit`** — it opens an interactive editor which agents cannot use.

---

## Phase 4: Verify Against the Issue

Re-read the issue and confirm every acceptance criterion has been addressed:

```bash
bd show {id} --json
```

Walk through the issue's description, acceptance criteria, and any linked context.
For each requirement, verify the corresponding change exists in your commits:

```bash
git log --oneline $DEFAULT_BRANCH..HEAD
git diff $DEFAULT_BRANCH --stat
```

**Feature-specific checks:**
- Does the implementation match the proposed behavior exactly?
- Are all acceptance criteria met — not just some?
- Does the new feature integrate cleanly with existing functionality?
- Are there edge cases in the acceptance criteria you haven't handled?

If anything is missing or only partially implemented, go back to Phase 3.
Do NOT proceed to merge until the issue is fully addressed — not "mostly done."

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

# Merge the feature branch
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
bd close {id} --reason "Completed: <specific summary of deliverables>" --json

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
