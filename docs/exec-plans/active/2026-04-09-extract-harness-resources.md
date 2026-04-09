# ExecPlan: Extract Harness Resources

## Objective
Extract the hook scripts from the `.codex/hooks/` directory to a public repository-level `hooks/` directory, and rename the tool-specific `docs/codex/` directory to `docs/harness/` to ensure the project structure is tool-agnostic.

## Current Context
- Python execution hooks currently live in `.codex/hooks/` which is inherently tied to the Codex CLI.
- Policies and documentation live in `docs/codex/`.
- Both sets of files are referenced by multiple configuration files (`.codex/hooks.json`, `.gemini/settings.json`), documentation (`AGENTS.md`, `PLANS.md`), and git hooks.

## Implementation Steps
1. **Rename Directories and Move Files:**
   - Move all Python scripts from `.codex/hooks/` to a new `hooks/` directory at the project root.
   - Rename `docs/codex/` to `docs/harness/`.
2. **Update Configuration Files:**
   - Update `.gemini/settings.json` to point to `./hooks/...` instead of `./.codex/hooks/...`.
   - Update `.codex/hooks.json` to point to `./hooks/...` instead of `./.codex/hooks/...`.
3. **Update Documentation References:**
   - Update references to `docs/codex/` and `.codex/hooks/` in `AGENTS.md`.
   - Update references to `docs/codex/` and `.codex/hooks/` in `PLANS.md`.
   - Update references to `docs/codex/` in the markdown files within the newly named `docs/harness/` folder (`README.md`, `EXECUTION_POLICY.md`, `CHANGELOG.md`, `LOCAL_RUNTIME.md`, `GIT_GATES.md`).
4. **Update Python Hook Script References:**
   - Check Python hook scripts inside the newly moved `hooks/` directory for any hardcoded references to `docs/codex/` (e.g., `user_prompt_submit_policy.py`) and update them to `docs/harness/`.
5. **Update `.githooks/pre-commit`:**
   - Ensure the git hook doesn't have broken paths due to the rename.
6. **Clean Up:**
   - Ensure `.codex/hooks/` is completely empty and removed if no longer necessary.

## Validation Plan
- Run tests on the configuration paths using a quick validation run or text search (`rg docs/codex` and `rg \.codex/hooks` should return 0 results).
- Ensure `.gemini/settings.json` and `.codex/hooks.json` contain the correctly updated paths.

## Review Gate Status
- **Review Status:** approved

## Progress Log
- [ ] Plan created.
