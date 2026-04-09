# Codex Harness Index

Use this directory as a short index, not as another policy document.

- `EXECUTION_POLICY.md`: runtime behavior rules
- `GIT_GATES.md`: commit gate and `pre-commit` behavior
- `LOCAL_RUNTIME.md`: local setup only
- `CHANGELOG.md`: harness history only

Runtime assets:

- `.codex/rules/default.rules`
- `.codex/hooks.json`
- `.codex/hooks/`
- `.githooks/pre-commit`

Limitations:

- Codex hooks are guardrails, not a full enforcement boundary.
- `PreToolUse` and `PostToolUse` currently cover Bash only.
- This repo does not commit `.codex/config.toml`.
