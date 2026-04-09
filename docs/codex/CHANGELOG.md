# Harness Changelog

## 2026-04-09

- Added the initial Codex harness skeleton for this repository.
- Added `AGENTS.md`, `PLANS.md`, and the `docs/codex/` policy set.
- Added repo-local Codex rules and hook scripts.
- Added a versioned Git `pre-commit` gate for Rust formatting and clippy checks.
- Added the human-reviewed commit gate: changed tasks now require a review package and explicit chat approval before `git commit`.
- Compressed the prompt-facing policy docs to reduce repeated guidance and token usage.
- Removed additional prompt duplication across the harness index and Git gate docs.
- Removed the `Final Commit SHA` requirement and simplified active-plan detection to use working-tree changes only.
