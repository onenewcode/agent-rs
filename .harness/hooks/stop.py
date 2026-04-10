#!/usr/bin/env python3
from __future__ import annotations

import sys
from typing import Any

from command_policy import (
    classify_command,
    raw_contains_prefix,
    scan_for_command,
    starts_with_prefix,
)
from decision_log import log_decision
from hook_types import Decision
from path_policy import matches_blocked_path
from policy_io import load_policy, read_stdin_payload


EVENT = "Stop"


def decide_stop(payload: dict[str, Any], policy: dict[str, Any]) -> Decision:
    commands = policy.get("commands", {})
    paths = policy.get("paths", {})
    enforcement = policy.get("enforcement", {})

    command = scan_for_command(payload)
    raw = payload.get("_raw_stdin", "")
    if not isinstance(raw, str):
        raw = ""
    classification = classify_command(command)

    blocked_prefixes = list(commands.get("blocked_prefixes", []))
    blocked_match = starts_with_prefix(command, blocked_prefixes) if command else None
    if blocked_match is None and raw:
        blocked_match = raw_contains_prefix(raw, blocked_prefixes)
    if blocked_match:
        return Decision(
            decision="deny",
            reason=f"Blocked destructive command prefix: {blocked_match}",
            matched_rules=[f"blocked_command:{blocked_match}"],
            classification="high_risk",
            command=command,
        )

    scope = enforcement.get("scope", "writes_and_high_risk_only")
    if scope == "writes_and_high_risk_only":
        if classification == "read":
            return Decision(
                decision="allow",
                reason="Read command allowed",
                matched_rules=["scope:writes_and_high_risk_only"],
                classification=classification,
                command=command,
            )
        if classification == "unknown":
            unknown_mode = enforcement.get("unknown_command", "allow_warn")
            if unknown_mode == "allow_warn":
                return Decision(
                    decision="warn",
                    reason="Unknown command classification allowed with warning",
                    matched_rules=["unknown_command:allow_warn"],
                    classification=classification,
                    command=command,
                )
            return Decision(
                decision="deny",
                reason="Unknown command blocked by policy",
                matched_rules=[f"unknown_command:{unknown_mode}"],
                classification=classification,
                command=command,
            )

    blocked_paths = list(paths.get("blocked_write_paths", [])) or list(paths.get("blocked_paths", []))
    if classification in {"write", "high_risk"}:
        blocked, detail = matches_blocked_path(command, blocked_paths)
        if blocked:
            return Decision(
                decision="deny",
                reason=f"Write to blocked path detected ({detail})",
                matched_rules=["blocked_path"],
                classification=classification,
                command=command,
            )

    return Decision(
        decision="allow",
        reason="Allowed by policy",
        matched_rules=["default_allow"],
        classification=classification,
        command=command,
    )


def main() -> int:
    payload = read_stdin_payload()
    policy = load_policy()
    decision = decide_stop(payload, policy)
    log_decision(EVENT, decision, policy)

    if decision.decision == "deny":
        print(decision.reason)
        return 2
    if decision.decision == "warn":
        print(decision.reason, file=sys.stderr)
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
