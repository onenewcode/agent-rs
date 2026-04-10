#!/usr/bin/env python3
from __future__ import annotations

import sys
from typing import Any

from command_policy import (
    classify_command,
    raw_contains_prefix,
    scan_for_command,
    split_command_segments,
    starts_with_prefix,
)
from hook_types import Decision
from path_policy import matches_blocked_path
from policy_io import load_policy, read_stdin_payload


DEFAULT_EVENT = "PreToolUse"
DEFAULT_PLATFORM = "unknown"
DECISION_RANK = {"allow": 0, "warn": 1, "deny": 2}
CLASSIFICATION_RANK = {"read": 0, "unknown": 1, "write": 2, "high_risk": 3}
UNKNOWN_MODES = {"allow_warn", "allow_silent", "deny"}
SUPPORTED_SCOPE = "writes_and_high_risk_only"


def _is_string_list(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(item, str) for item in value)


def _normalize_prefixes(values: list[str]) -> list[str]:
    out: list[str] = []
    for value in values:
        normalized = value.strip()
        if normalized:
            out.append(normalized)
    return out


def _resolve_policy(policy: dict[str, Any]) -> tuple[dict[str, Any] | None, str | None]:
    commands = policy.get("commands")
    if not isinstance(commands, dict):
        return None, "invalid_policy:missing_commands"

    paths = policy.get("paths")
    if not isinstance(paths, dict):
        return None, "invalid_policy:missing_paths"

    enforcement = policy.get("enforcement")
    if not isinstance(enforcement, dict):
        return None, "invalid_policy:missing_enforcement"

    scope = enforcement.get("scope")
    if not isinstance(scope, str) or scope != SUPPORTED_SCOPE:
        return None, "invalid_policy:unsupported_scope"

    unknown_mode = enforcement.get("unknown_command")
    if not isinstance(unknown_mode, str) or unknown_mode not in UNKNOWN_MODES:
        return None, "invalid_policy:unknown_command_mode"

    allowed_prefixes = commands.get("allowed_prefixes")
    write_prefixes = commands.get("write_prefixes")
    blocked_prefixes = commands.get("blocked_prefixes")
    high_risk_prefixes = commands.get("high_risk_prefixes")
    blocked_write_paths = paths.get("blocked_write_paths")

    if not _is_string_list(allowed_prefixes):
        return None, "invalid_policy:allowed_prefixes"
    if not _is_string_list(write_prefixes):
        return None, "invalid_policy:write_prefixes"
    if not _is_string_list(blocked_prefixes):
        return None, "invalid_policy:blocked_prefixes"
    if not _is_string_list(high_risk_prefixes):
        return None, "invalid_policy:high_risk_prefixes"
    if not _is_string_list(blocked_write_paths):
        return None, "invalid_policy:blocked_write_paths"

    return (
        {
            "allowed_prefixes": _normalize_prefixes(allowed_prefixes),
            "write_prefixes": _normalize_prefixes(write_prefixes),
            "blocked_prefixes": _normalize_prefixes(blocked_prefixes),
            "high_risk_prefixes": _normalize_prefixes(high_risk_prefixes),
            "blocked_write_paths": _normalize_prefixes(blocked_write_paths),
            "scope": scope,
            "unknown_mode": unknown_mode,
        },
        None,
    )


def _overall_classification(classifications: list[str], command: str, cfg: dict[str, Any]) -> str:
    if not classifications:
        return classify_command(
            command,
            allowed_prefixes=cfg["allowed_prefixes"],
            write_prefixes=cfg["write_prefixes"],
            high_risk_prefixes=cfg["high_risk_prefixes"],
        )
    return max(classifications, key=lambda item: CLASSIFICATION_RANK.get(item, 0))


def _dedupe(values: list[str]) -> list[str]:
    seen: set[str] = set()
    out: list[str] = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        out.append(value)
    return out


def decide_event(event: str, payload: dict[str, Any], policy: dict[str, Any]) -> Decision:
    cfg, err = _resolve_policy(policy)
    if err:
        return Decision(
            decision="deny",
            reason=err,
            matched_rules=["invalid_policy"],
            classification="unknown",
            command=scan_for_command(payload),
            metadata={"event": event},
        )
    assert cfg is not None

    command = scan_for_command(payload)
    raw = payload.get("_raw_stdin", "")
    if not isinstance(raw, str):
        raw = ""

    segments = split_command_segments(command)
    if command.strip() and not segments:
        segments = [command.strip()]

    blocked_prefixes = cfg["blocked_prefixes"]
    blocked_paths = cfg["blocked_write_paths"]
    scope = cfg["scope"]
    unknown_mode = cfg["unknown_mode"]

    current_decision = "allow"
    current_reason = ""
    matched_rules: list[str] = []
    segment_classifications: list[str] = []

    def mark(decision: str, code: str, rule: str) -> None:
        nonlocal current_decision, current_reason
        matched_rules.append(rule)
        if DECISION_RANK[decision] > DECISION_RANK[current_decision]:
            current_decision = decision
            current_reason = code

    for segment in segments:
        classification = classify_command(
            segment,
            allowed_prefixes=cfg["allowed_prefixes"],
            write_prefixes=cfg["write_prefixes"],
            high_risk_prefixes=cfg["high_risk_prefixes"],
        )
        segment_classifications.append(classification)

        blocked_match = starts_with_prefix(segment, blocked_prefixes)
        if blocked_match:
            mark(
                "deny",
                f"blocked_prefix:{blocked_match}",
                f"blocked_command:{blocked_match}",
            )
            continue

        if classification == "read":
            matched_rules.append("scope:writes_and_high_risk_only")
        elif classification == "unknown":
            if unknown_mode == "allow_warn":
                mark("warn", "unknown_command", "unknown_command:allow_warn")
            elif unknown_mode == "allow_silent":
                matched_rules.append("unknown_command:allow_silent")
            else:
                mark("deny", "unknown_command", "unknown_command:deny")

        if classification in {"write", "high_risk"}:
            blocked, detail = matches_blocked_path(segment, blocked_paths)
            if blocked:
                mark(
                    "deny",
                    f"blocked_path:{detail}",
                    "blocked_path",
                )

    if not segments and raw:
        blocked_match = raw_contains_prefix(raw, blocked_prefixes)
        if blocked_match:
            mark(
                "deny",
                f"blocked_prefix:{blocked_match}",
                f"blocked_command:{blocked_match}",
            )

    if not matched_rules:
        matched_rules.append("default_allow")

    return Decision(
        decision=current_decision,
        reason=current_reason,
        matched_rules=_dedupe(matched_rules),
        classification=_overall_classification(segment_classifications, command, cfg),
        command=command,
        metadata={
            "segments": segments,
            "scope": scope,
            "unknown_command_mode": unknown_mode,
        },
    )


def run_event(event: str, platform: str = DEFAULT_PLATFORM) -> int:
    payload = read_stdin_payload()
    policy = load_policy()
    decision = decide_event(event, payload, policy)

    if decision.decision == "deny":
        print(f"DENY {decision.reason}")
        return 2
    if decision.decision == "warn":
        print(f"WARN {decision.reason}", file=sys.stderr)
        return 0
    return 0


def main() -> int:
    event = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_EVENT
    platform = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_PLATFORM
    if event not in {"PreToolUse", "PostToolUse", "Stop"}:
        print(f"Unsupported hook event: {event or 'Unknown'}", file=sys.stderr)
        return 1
    return run_event(event, platform)


if __name__ == "__main__":
    raise SystemExit(main())
