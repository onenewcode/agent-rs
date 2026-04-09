# ExecPlan: execplan-cleanup

## Objective

Remove the `Final Commit SHA` requirement and use working-tree state to detect the active ExecPlan.

## Context

- The current harness still treats `Final Commit SHA` as required in the template and stop hook.
- The user does not want commit SHAs written back into plans.
- The user does not want a separate `docs/exec-plans/completed/` archive.

## Decisions

- Finished plans may stay in `docs/exec-plans/active/` as immutable history.
- Hooks treat only dirty plan files as active work.
- Completion is determined by review state and working tree state, not by writing SHA metadata into plans.
- Already-committed ExecPlans stay immutable; do not rewrite old files to fit new policy.

## Steps

1. Remove SHA references from docs and hooks.
2. Replace archive and naming rules with working-tree-based active-plan detection.
3. Leave the old bootstrap plan unchanged and make hook discovery ignore clean historical plans.
4. Validate hook behavior and updated plan discovery.

## Validation

- `rg -n "Final Commit SHA|completed/" AGENTS.md PLANS.md docs .codex`
- Python hook syntax checks
- sample hook runs for `stop_continue.py` and `pre_tool_use_policy.py`

## Review Gate

- Review Status: approved
- Files Intended For Commit: `PLANS.md`, `docs/codex/`, `docs/exec-plans/active/`, `.codex/hooks/`
- Proposed Commit Message: `Simplify ExecPlan completion rules`
- Human Approval: approved in chat

## Progress Log

- 2026-04-09: started the cleanup for SHA-free plan completion and simpler active-plan rules.
- 2026-04-09: updated active-plan discovery to prefer dirty plan files so immutable history is not mistaken for active work.

## Risks / Follow-Ups

- Existing hook behavior must keep blocking commit without human approval.
- Clean historical plans in `active/` now rely on git status to stay out of active-plan discovery.
