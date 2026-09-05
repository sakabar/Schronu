---
name: orchestrating-backlog-waves
description: Use when coordinating a backlog.md Wave across sidebar-visible Codex tasks, branches, and worktrees in the current saved project.
---

# Orchestrating Backlog Waves

## Overview

Coordinate exactly one `backlog.md` Wave through explicit gates. Invoke as `$orchestrating-backlog-waves N`; never execute Wave `N+1`.

## Required Protocol

Before creating a task, read and follow [references/orchestration-protocol.md](references/orchestration-protocol.md) completely. Stricter repository instructions remain authoritative.

## Non-negotiable Boundaries

- Automatic selection grants no task-creation authority. Require an explicit request for lane tasks.
- Create each lane with `create_thread` in the current saved project and a separate worktree. Internal subagents are only for reviews.
- A lane's first turn is investigation and a contract-level commit plan only. It must not edit files, implement, create commits, or push. "Safe" tests, scaffolding, and partial implementation are still forbidden.
- A hard dependency is ready only when its required contract commit is on `main`. Do not bypass this with a stacked branch, draft PR, or inferred API.
- A lane may write only reserved files. Stop it for new scope, a shared contract, or a changed Red reason; continue independent lanes.
- Lane self-reports do not authorize publication. Parent review and the applicable combined verification must pass before push or PR.
- Create non-draft PRs but never merge them. Report Wave `N+1` only as remaining work.

## Quick Reference

| Gate | Required evidence | Next action |
| --- | --- | --- |
| Discovery | Unambiguous Wave and reservations | Create planning tasks |
| Plan | Complete plan and unchanged worktree | Approve ready lanes |
| Lane Green | Red/Green history, internal reviews, quality gates | Parent branch review |
| Parent review | Allowed diff, maintainable cumulative change, no unresolved P1/P2 | Combined verification |
| Combined Green | Dependency-order composition and Wave gates pass | Push and create non-draft PRs |

## Red Flags

Stop the affected lane if anyone proposes:

- implementation during the first turn because it is small or reversible;
- starting from an unmerged dependency branch;
- same-file ownership because edits use different lines;
- treating an internal review or a Green self-report as parent approval;
- publishing before combined verification;
- implementing the next Wave "while waiting."

If a lane violated the first-turn prohibition, isolate its work and ask the user how to recover. Do not silently salvage, reset, revert, archive, or recreate it.

## Common Rationalizations

| Rationalization | Required response |
| --- | --- |
| "The dependency branch is already Green." | Wait until the required commit is on `main`. |
| "Only ten shared lines change." | Serialize ownership of the whole file. |
| "The lane already spent hours and tests pass." | Preserve the state, withhold approval, and ask the user. |
| "A draft PR keeps momentum." | Planning tasks and dependency waits are not publication gates. |
| "The next Wave is independent." | It is outside this invocation's execution scope. |
