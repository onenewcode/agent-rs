#!/usr/bin/env python3

import json
import sys


BLOCK_PATTERNS = (
    "skip plan",
    "skip planning",
    "don't make a plan",
    "do not make a plan",
    "直接改代码",
    "别做计划",
)

IMPLEMENT_HINTS = (
    "implement",
    "edit",
    "refactor",
    "fix",
    "write code",
    "修改",
    "编码",
)


def main() -> int:
    payload = json.load(sys.stdin)
    prompt = (payload.get("prompt") or payload.get("user_prompt") or "").lower()

    if any(pattern in prompt for pattern in BLOCK_PATTERNS):
        json.dump(
            {
                "decision": "block",
                "reason": "This repository requires plan-first execution for non-trivial work.",
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "This repository requires plan-first execution for non-trivial work.",
                }
            },
            sys.stdout,
        )
        return 0

    if any(pattern in prompt for pattern in IMPLEMENT_HINTS):
        msg = (
            "For non-trivial work, read AGENTS.md and PLANS.md first. "
            "Create or update an ExecPlan under docs/exec-plans/active/, "
            "then wait for explicit user confirmation before editing files. "
            "If the task ends with repo changes, prepare a review package and "
            "wait for explicit human approval before git commit."
        )
        json.dump(
            {
                "decision": "allow",
                "systemMessage": msg,
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": msg,
                }
            },
            sys.stdout,
        )
        return 0

    json.dump({"decision": "allow"}, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
