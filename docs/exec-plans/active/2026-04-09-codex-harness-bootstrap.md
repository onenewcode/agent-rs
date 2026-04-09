# ExecPlan: codex-harness-bootstrap

## Objective

Bootstrap a repo-local Codex CLI harness with plan-first workflow, traceability, Codex guardrails, and Git commit gates.

## Context

- Minimal Rust crate at start.
- Constrain Codex before real coding.
- Do not commit `.codex/config.toml`.

## Decisions

- Keep runtime guidance in Markdown, not project-level CLI config.
- Use repo-local Codex rules and hooks.
- Use a real Git `pre-commit` hook for `cargo fmt` and `cargo clippy`.
- Keep this plan active until the harness is committed.

## Steps

1. Create the harness policy docs and traceability layout.
2. Add Codex rules and repo-local hook scripts.
3. Add a versioned Git pre-commit hook for Rust formatting and clippy.
4. Validate script syntax and local hook installation.

## Validation

- Python hook syntax checks
- `bash -n .githooks/pre-commit`
- `codex execpolicy check` on representative allowed, prompt, and forbidden commands
- `wc -w` on prompt-facing policy docs after compression

## Review Gate

- Review Status: approved
- Files Intended For Commit: `AGENTS.md`, `PLANS.md`, `docs/codex/`, `docs/exec-plans/`, `.codex/`, `.githooks/pre-commit`
- Proposed Commit Message: `bootstrap rust repository`
- Human Approval: approved in chat
- Final Commit SHA:

## Progress Log

- 2026-04-09: bootstrapped the harness, added the human-reviewed commit gate, and compressed prompt-facing docs.

## Risks / Follow-Ups

- Repo-local Codex hooks require the local `codex_hooks` feature to be enabled.
- Codex hook coverage is not a full security boundary because Bash interception is narrower than general tool interception.
- Move this file to `docs/exec-plans/completed/` after the harness is committed.
