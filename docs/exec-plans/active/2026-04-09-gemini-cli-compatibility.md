# ExecPlan: Gemini CLI Compatibility Refactor

## Objective
Refactor the repository's hook configurations and Python enforcement scripts so they operate correctly under both the Codex CLI and the Gemini CLI, satisfying the current project constraints.

## Current Context
- The project currently relies on `.codex/hooks.json` which defines `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, and `Stop` hook events.
- These events trigger Python scripts in `.codex/hooks/` to enforce execution policies (e.g., plan-first workflow, pre-commit validation, etc.).
- The Python scripts are currently tightly coupled to the Codex CLI's payload schema (both input from `stdin` and output to `stdout`).

## Implementation Steps
1. **Create Gemini CLI Configuration:**
   - Create `.gemini/hooks.json` mapped to the corresponding Gemini CLI hook events (`BeforeAgent`, `BeforeTool`, `AfterTool`, `Stop`/`SessionEnd`).
   - Point these configurations to the existing Python scripts in `.codex/hooks/`.
2. **Update `user_prompt_submit_policy.py`:**
   - Support reading both `prompt` (Codex) and `user_prompt` (Gemini) from the input payload.
   - Output a unified JSON containing both `decision: "block"/"allow"` (for Gemini) and the Codex-specific `hookSpecificOutput` structure.
3. **Update `pre_tool_use_policy.py`:**
   - Update to parse tool parameters from both CLIs.
   - Return combined denial responses including `{"decision": "deny", "reason": "..."}`.
4. **Update `post_tool_use_review.py` & `stop_continue.py`:**
   - Adapt payload parsing and emit universally compatible system messages.
5. **Update `.codex/rules/default.rules` (If applicable):**
   - Symlink or copy to `.gemini/rules/default.rules` if needed for Gemini CLI compatibility.

## Validation Plan
- Verify `.gemini/hooks.json` is correctly placed and readable.
- Test the scripts manually with sample JSON inputs representing both Codex and Gemini CLI payloads to ensure the output schemas are well-formed.
- If applicable, run a quick Gemini CLI test command to trigger the `BeforeAgent` hook and verify it executes the Python script successfully.

## Progress Log
- [x] Created `.gemini/settings.json` mapping Gemini CLI hook events (`BeforeAgent`, `BeforeTool`, `AfterTool`, `SessionEnd`).
- [x] Updated `user_prompt_submit_policy.py` to parse both `prompt` and `user_prompt`, and output Gemini `decision` keys.
- [x] Updated `pre_tool_use_policy.py` to parse tool inputs safely and return combined schemas.
- [x] Updated `post_tool_use_review.py` and `stop_continue.py` to support `context.cwd` or `cwd` alongside merging expected JSON formats.
- [x] Updated `docs/codex/CHANGELOG.md` to reflect the changes made.
- [x] Validation plan executed and complete.

## Review Gate Status
- **Review Status:** approved

## Remaining Risks
- Gemini CLI hook event names might differ slightly depending on the exact version; the configuration will be adjusted if execution fails.