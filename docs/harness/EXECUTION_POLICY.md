# Execution Policy

This is the runtime policy for this repository.

## Allowed Without Extra Confirmation

- read files
- search the repository
- inspect diffs and git status
- run low-risk validation: `cargo check`, `cargo test`, `cargo clippy`
- explain findings, plans, and risks

## Requires Explicit Confirmation

- editing repository files for non-trivial tasks
- changing dependencies
- running `cargo fmt` directly as part of implementation
- running networked shell commands
- mutating git state with `git push`, branch creation, or tag creation
- broad refactors or generated-file updates
- changing harness assets under `.codex/`, `.githooks/`, `AGENTS.md`, `PLANS.md`, or `docs/harness/`

`git add` is allowed during post-validation commit preparation. `git commit` still requires explicit human approval in chat and that approval must be recorded in the active ExecPlan before the command runs.

## Forbidden

- `git commit --no-verify` or `git commit -n`
- destructive delete commands
- `git reset --hard`
- `git clean -fd` or `git clean -fdx`
- history rewriting commands such as force-push
- bypassing the plan-first workflow on non-trivial tasks
- finishing a changed task without entering review-and-commit preparation

## Notes

- Technical ability is not permission.
- For non-trivial work, an active ExecPlan is required before coding starts.
- For tasks with repo changes, the active ExecPlan must record one of: `pending_human_review`, `approved`, or `deferred`.
- Hooks only treat changed ExecPlan files in the working tree as active work.
- Harness changes must update `docs/harness/CHANGELOG.md`.
