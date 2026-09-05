# Wave Orchestration Protocol

## 1. Discover and Reserve

1. Parse `N` as a non-negative integer. Locate the exact `Wave N` section, every `WN-*` lane, its backlog item, contract, dependencies, write range, Wave notes, integration gates, and the `Wave N+1` section when present.
2. Stop before task creation if lane membership, dependency kind, completion contract, or write ownership is ambiguous. Do not invent missing policy from neighboring Waves.
3. Confirm that the user explicitly requested new sidebar-visible lane tasks. Automatic Skill selection or a request to explain/plan the Wave is not authorization to call `create_thread`; ask first when that authority is absent.
4. Read repository instructions and inspect `main`, its revision, current worktrees, remotes, and relevant backlog items. A status label alone does not satisfy a dependency; verify that the required contract commit exists on `main`.
5. Build a reservation table for product files, tests, fixtures, and documentation. The whole file is the ownership unit. Create all lane planning tasks, but authorize implementation only for dependency-ready lanes.
6. Resolve the saved project with `list_projects`: match the primary worktree path from `git worktree list` to a Git repository project. If no unique match exists, stop and ask the user. Never create a projectless substitute.

## 2. Create Planning Tasks

For each lane, call `create_thread` with the resolved `projectId` and `environment.type: worktree`. Omit model and thinking overrides. Use a title containing the lane ID and backlog item.

If worktree setup returns only a `clientThreadId`, do not pass it to tools that require a ready `threadId`. Use the task listing to resolve the ready task before waiting, reading, or messaging it. Track the ready task ID, host ID, worktree path, intended branch, dependencies, and reservation state in the parent lane table.

The initial prompt must include:

- Wave/lane ID, backlog item, fixed contract, dependencies, reserved write range, base `main` revision, and intended `feature/<lane>-<short-name>` branch;
- an absolute prohibition on file edits, implementation, branch creation, commits, push, and PR creation during the first turn;
- a request to inspect `backlog.md`, `AGENTS.md`, relevant code/tests, dependency readiness, and write conflicts;
- a required commit plan for every contract unit: commit message, fixed contract, responsibility/module, dependency on earlier commits, target tests, expected single Red reason, and Green command;
- an instruction to end the turn after presenting the plan and wait for parent approval.

Wait with `wait_threads`; split more than eight lanes into groups of at most eight targets and use each returned cursor for later waits. Use `read_thread` only for missing detail, attention requests, or final evidence. Confirm from the lane worktree that the first turn added no commit and changed no file. If it did, isolate the lane and ask the user for recovery direction while other lanes continue.

## 3. Approve and Monitor Implementation

Review each plan against the backlog contract, repository commit rules, dependencies, and reservation table. Send an explicit implementation-start message only when both the plan and dependencies are approved. Require the lane to create or confirm its dedicated feature branch before editing.

Each lane must:

1. Add one contract-level Red test, run it, and confirm one expected failure reason before committing it.
2. Add the smallest Green implementation, run target tests and repository quality gates, and commit the basic Green implementation so the review target is stable.
3. Obtain an internal subagent specification/code-quality review after that Green commit. Fix findings one at a time with target verification and one finding per commit. Do not dismiss P1/P2 findings or weaken earlier tests.
4. Review `main...branch` cumulatively for responsibility boundaries, duplication, file size, fixtures, failure paths, dependency direction, and repository-specific maintainability thresholds.
5. Update only its own backlog status/detail/verification/residual-work text in a separate documentation commit after product Green and after the parent grants that lane the exclusive `backlog.md` documentation lease. Release the lease after the commit. Grant leases in dependency and intended PR merge order; do not edit shared global verification summaries or another lane's item.
6. Finish with a clean worktree, full quality gates, `git diff --check`, and a history review. Do not push or create a PR yet.

If a lane needs an unreserved file, public API/schema/error change, or discovers a different Red reason, tell it to preserve its state and stop that expansion. Review the impact and obtain explicit parent or user approval before updating reservations and the commit plan. Never use artificial calls, test-only production branches, reduced error information, or duplicated helpers to remain inside an obsolete reservation.

## 4. Parent Review and Combined Gate

For each completed lane, independently inspect:

- `git log --reverse` with subjects and bodies for the lane range;
- `git diff --stat`, `--numstat`, `--name-only`, `--check`, and the full `main...branch` diff;
- Red/Green pairing, single-purpose commits, review-fix isolation, allowed files, preserved tests, product-path coverage, documentation isolation, and clean status;
- repository thresholds such as files or cumulative additions over 800 lines, oversized fixtures, repeated helpers/error mappings, or responsibilities that cannot be followed locally.

Send concrete findings back to the same lane. Require P1/P2 fixes and re-review; record uncertain findings without blocking safe fixes. A Green test suite does not override a maintainability or history failure.

For lanes without an intra-Wave hard dependency, wait until every lane passes parent review, then create a disposable integration worktree from current `main`. Apply lane commits in dependency and documented serialization order without modifying feature branches. Resolve no semantic conflict silently: route it to the owning lane, have that lane rebase or repair its branch, repeat parent review, then rebuild the disposable integration state.

When a lane has an intra-Wave hard dependency, process the Wave by dependency frontier instead of deadlocking. Parent-review every ready frontier, combine current `main` with that frontier and all previously merged Wave work, run the applicable gates, then publish only the frontier PRs. Stop dependent implementation until the user merges the prerequisite PRs to `main`; verify the merge commit, rebase dependent lanes, and continue. After the final frontier, run the full-Wave combined gate before publishing its PRs. Never merge a prerequisite PR on the user's behalf.

Run `git diff --check`, repository-wide format/lint/test gates, Wave-specific gates, and cross-boundary integration tests required by the backlog. Remove or leave the disposable worktree according to safe repository practice; never publish its synthetic history.

If the selected Wave requires a shared backlog summary, update it only after the full-Wave combined gate. Use the backlog-designated lane; otherwise assign the lexicographically last lane in the final dependency frontier. Do not create another task. Give that existing lane the exclusive documentation lease and require a separate final docs commit containing only the shared summary. Parent-review the commit, rebuild the disposable integration state, and rerun `git diff --check`, documentation-specific checks, and any repository gate whose inputs include the changed documentation before publication.

## 5. Publish and Report

Only after the required parent reviews and combined gate pass, tell eligible lanes to push their feature branches normally and create non-draft PRs targeting `main`. For an intra-Wave dependency, "eligible" means the current dependency frontier; otherwise it means all lanes. Require the PR body to state purpose, contract changes, Red/Green history, review fixes, verification, compatibility, dependencies, merge order, and residual work. Do not merge, force-push, or publish a combined integration branch.

Because lanes take serialized leases for separate backlog documentation commits, state the PR merge order. After an earlier PR is merged, later conflicting branches must rebase on updated `main`, preserve already-merged backlog evidence, rerun relevant and full gates, and receive renewed parent review before merge readiness is claimed. A required shared summary stays on its assigned existing lane.

The final report must list each lane's task title/ID, branch, commits, review status, gates, PR URL/state, required merge/rebase order, and any blocked or unapproved scope. Distinguish "development and PR preparation complete" from "merged to main." Describe Wave `N+1` dependencies and residual work, but do not create its tasks, branches, commits, or PRs.

## Example Initial Prompt

```text
You own W4-A / TD-039 for planning only. The fixed contract, dependency list, reserved product/test/fixture/docs files, intended branch, and base main revision are included below.

In this first turn, do not edit files, implement, create or switch branches, commit, push, or create a PR. Inspect backlog.md, AGENTS.md, dependencies, and the relevant code/tests. Return a contract-level Red/Green commit plan containing, for every commit: message, fixed contract, responsibility/module, dependency, target test, expected single Red reason, and Green verification command. End the turn after the plan and wait for explicit parent approval.
```
