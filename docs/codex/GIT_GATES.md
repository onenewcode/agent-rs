# Git Gates

This file covers Git-specific gates only.

## Hook Location

- Versioned hook scripts live under `.githooks/`
- The repository expects `git config core.hooksPath .githooks`

## Pre-Commit Behavior

The `pre-commit` hook only runs Rust gates when staged changes include:

- `*.rs`
- `Cargo.toml`
- `Cargo.lock`

When triggered, it:

1. rejects the commit if there are unstaged Rust-related edits, because auto-formatting would mix staged and unstaged work
2. runs `cargo fmt`
3. auto-stages formatting changes if and only if formatting touched files that were already part of the staged Rust-related set
4. stops the commit if formatting changed additional files outside that staged set
5. runs `cargo clippy --workspace --all-targets --all-features`
6. blocks the commit on clippy errors

Warnings do not fail the commit in v1.

## Human Review Gate

Before `git commit`, present:

- changed files or diff summary
- validation run
- unresolved risks
- proposed commit message

`git add` is allowed after validation. `git commit` still requires explicit human approval in chat and that approval must be recorded in the active ExecPlan.

## Bypass Policy

- `git commit --no-verify` is prohibited for normal work.
- Use it only for harness recovery or emergency unblocking, and document the reason in the active ExecPlan or handoff notes.
