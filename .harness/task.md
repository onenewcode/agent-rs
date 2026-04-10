# Harness Task

Keep Codex, Claude, and Gemini aligned around one shared task file and one shared policy file.

Current goals:
- Generate each platform's instruction and hook/config artifacts from `.harness/policy.toml` and `.harness/task.md`.
- Enforce command, path, and diff-budget checks through one Python hook implementation.
- Write structured hook decisions into `.harness/logs/*.jsonl`.

Constraints:
- Use Python only.
- Do not introduce a new runtime CLI.
- Do not implement sandboxing.
- Keep the first version simple and explicit.
