#!/usr/bin/env python3

import json
import subprocess
import sys
from pathlib import Path

from execplan_review import RECOGNIZED_REVIEW_STATES, get_active_exec_plan, parse_execplan_review


def git_has_changes(repo_root: Path) -> bool:
    result = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    return bool(result.stdout.strip())


def main() -> int:
    payload = json.load(sys.stdin)
    repo_root = Path(payload.get("cwd") or payload.get("context", {}).get("cwd") or ".")

    if payload.get("stop_hook_active"):
        json.dump({"decision": "allow", "continue": True}, sys.stdout)
        return 0

    has_changes = git_has_changes(repo_root)
    active_plan = get_active_exec_plan(repo_root)

    if has_changes and active_plan is None:
        json.dump(
            {
                "decision": "deny",
                "reason": "Repo changes require an active ExecPlan.",
                "continue": False,
                "stopReason": "Repo changes require an active ExecPlan.",
                "systemMessage": (
                    "This repository has changes but no active ExecPlan file under "
                    "docs/exec-plans/active/. Create or update a plan before ending the turn."
                ),
            },
            sys.stdout,
        )
        return 0

    if has_changes and active_plan is not None:
        review = parse_execplan_review(active_plan)
        review_status = review.get("Review Status", "")

        if review_status not in RECOGNIZED_REVIEW_STATES:
            json.dump(
                {
                    "decision": "deny",
                    "reason": "Changed work requires review status in the active ExecPlan.",
                    "continue": False,
                    "stopReason": "Changed work requires review status in the active ExecPlan.",
                    "systemMessage": (
                        f"Active ExecPlan {active_plan} must record a valid Review Status "
                        "such as pending_human_review, approved, or deferred before ending the turn."
                    ),
                },
                sys.stdout,
            )
            return 0

        if review_status == "approved":
            json.dump(
                {
                    "decision": "deny",
                    "reason": "Approved work should be committed or explicitly deferred.",
                    "continue": False,
                    "stopReason": "Approved work should be committed or explicitly deferred.",
                    "systemMessage": (
                        f"Active ExecPlan {active_plan} is approved for commit. "
                        "Run the commit or change the review state before ending the turn."
                    ),
                },
                sys.stdout,
            )
            return 0

    json.dump({"decision": "allow", "continue": True}, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
