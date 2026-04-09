#!/usr/bin/env python3

import json
import re
import sys
from pathlib import Path

from execplan_review import RECOGNIZED_REVIEW_STATES, get_active_exec_plan, parse_execplan_review


DENY_PATTERNS = (
    (r"\bgit\s+reset\s+--hard\b", "Hard reset is forbidden in this repository."),
    (r"\bgit\s+clean\s+-fdx?\b", "Destructive git clean is forbidden in this repository."),
    (r"\bgit\s+commit\b.*(?:--no-verify|-n)\b", "Do not bypass git hooks with --no-verify."),
    (r"\brm\s+-rf\b", "Recursive deletion is forbidden without explicit approval."),
)


def main() -> int:
    payload = json.load(sys.stdin)
    repo_root = Path(payload.get("cwd") or ".")
    command = (
        payload.get("tool_input", {}).get("command")
        or payload.get("tool_input.command")
        or ""
    )

    for pattern, reason in DENY_PATTERNS:
        if re.search(pattern, command):
            json.dump(
                {
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    },
                    "systemMessage": reason,
                },
                sys.stdout,
            )
            return 0

    if re.search(r"\bgit\s+commit\b", command):
        plan_path = get_active_exec_plan(repo_root)
        if plan_path is None:
            reason = "git commit requires an active ExecPlan with recorded human approval."
            json.dump(
                {
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    },
                    "systemMessage": reason,
                },
                sys.stdout,
            )
            return 0

        review = parse_execplan_review(plan_path)
        review_status = review.get("Review Status", "")
        if review_status not in RECOGNIZED_REVIEW_STATES:
            reason = (
                f"Active ExecPlan {plan_path} is missing a valid Review Status. "
                "Record pending_human_review, approved, or deferred before commit."
            )
            json.dump(
                {
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    },
                    "systemMessage": reason,
                },
                sys.stdout,
            )
            return 0

        if review_status != "approved":
            reason = (
                f"git commit requires explicit human approval recorded in {plan_path}. "
                f"Current Review Status: {review_status or 'missing'}."
            )
            json.dump(
                {
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    },
                    "systemMessage": reason,
                },
                sys.stdout,
            )
            return 0

        json.dump(
            {
                "systemMessage": (
                    "Git commits must respect .githooks/pre-commit and should only happen "
                    "after validation, human review approval, and ExecPlan updates."
                )
            },
            sys.stdout,
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
