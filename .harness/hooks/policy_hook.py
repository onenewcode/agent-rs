#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
import time
from typing import Any

from command_policy import (
    classify_command,
    raw_contains_prefix,
    scan_for_command,
    split_command_segments,
    starts_with_prefix,
)
from decision_log import log_decision
from hook_types import Decision
from path_policy import matches_blocked_path
from policy_io import ROOT, load_policy, read_stdin_payload


DEFAULT_EVENT = "PreToolUse"
DEFAULT_PLATFORM = "unknown"
DECISION_RANK = {"allow": 0, "warn": 1, "deny": 2}
CLASSIFICATION_RANK = {"read": 0, "unknown": 1, "write": 2, "high_risk": 3}


def _overall_classification(classifications: list[str], command: str) -> str:
    if not classifications:
        return classify_command(command)
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


def _collect_diff_budget_metrics() -> tuple[dict[str, int], str | None]:
    commands = [
        ["git", "diff", "--numstat", "HEAD"],
        ["git", "diff", "--numstat"],
    ]
    output = ""
    command_succeeded = False
    for cmd in commands:
        result = subprocess.run(
            cmd,
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0:
            command_succeeded = True
            output = result.stdout
            break

    if not command_succeeded:
        return {"files_changed": 0, "lines_added": 0, "lines_deleted": 0}, "git_diff_unavailable"

    files_changed = 0
    lines_added = 0
    lines_deleted = 0
    for line in output.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        files_changed += 1
        add_text, del_text = parts[0], parts[1]
        if add_text.isdigit():
            lines_added += int(add_text)
        if del_text.isdigit():
            lines_deleted += int(del_text)

    return {
        "files_changed": files_changed,
        "lines_added": lines_added,
        "lines_deleted": lines_deleted,
    }, None


def _evaluate_diff_budget(
    event: str,
    classifications: list[str],
    policy: dict[str, Any],
) -> tuple[str | None, list[str], dict[str, Any]]:
    budget = policy.get("diff_budget", {})
    enabled = bool(budget.get("enabled", False))

    metadata: dict[str, Any] = {"diff_budget_enabled": enabled}
    if not enabled:
        return None, ["diff_budget:disabled"], metadata
    if event not in {"PostToolUse", "Stop"}:
        metadata["diff_budget_status"] = "skipped_event"
        return None, ["diff_budget:skipped_event"], metadata
    if not any(item in {"write", "high_risk"} for item in classifications):
        metadata["diff_budget_status"] = "skipped_non_write"
        return None, ["diff_budget:skipped_non_write"], metadata

    metrics, error = _collect_diff_budget_metrics()
    metadata["diff_budget_metrics"] = metrics
    if error:
        metadata["diff_budget_status"] = error
        return None, ["diff_budget:metrics_unavailable"], metadata

    max_files_changed = int(budget.get("max_files_changed", 999999))
    max_lines_added = int(budget.get("max_lines_added", 99999999))
    max_lines_deleted = int(budget.get("max_lines_deleted", 99999999))
    metadata["diff_budget_limits"] = {
        "max_files_changed": max_files_changed,
        "max_lines_added": max_lines_added,
        "max_lines_deleted": max_lines_deleted,
    }

    exceeded: list[str] = []
    if metrics["files_changed"] > max_files_changed:
        exceeded.append("max_files_changed")
    if metrics["lines_added"] > max_lines_added:
        exceeded.append("max_lines_added")
    if metrics["lines_deleted"] > max_lines_deleted:
        exceeded.append("max_lines_deleted")

    if exceeded:
        metadata["diff_budget_status"] = "exceeded"
        reason = "Diff budget exceeded: " + ", ".join(exceeded)
        return reason, [f"diff_budget:{item}" for item in exceeded], metadata

    metadata["diff_budget_status"] = "within_budget"
    return None, ["diff_budget:within_budget"], metadata


def decide_event(event: str, payload: dict[str, Any], policy: dict[str, Any]) -> Decision:
    commands = policy.get("commands", {})
    paths = policy.get("paths", {})
    enforcement = policy.get("enforcement", {})

    command = scan_for_command(payload)
    raw = payload.get("_raw_stdin", "")
    if not isinstance(raw, str):
        raw = ""

    segments = split_command_segments(command)
    if command.strip() and not segments:
        segments = [command.strip()]

    blocked_prefixes = list(commands.get("blocked_prefixes", []))
    blocked_paths = list(paths.get("blocked_write_paths", [])) or list(paths.get("blocked_paths", []))
    scope = enforcement.get("scope", "writes_and_high_risk_only")
    unknown_mode = enforcement.get("unknown_command", "allow_warn")

    current_decision = "allow"
    current_reason = "Allowed by policy"
    matched_rules: list[str] = []
    segment_classifications: list[str] = []

    def mark(decision: str, reason: str, rule: str) -> None:
        nonlocal current_decision, current_reason
        matched_rules.append(rule)
        if DECISION_RANK[decision] > DECISION_RANK[current_decision]:
            current_decision = decision
            current_reason = reason

    for segment in segments:
        classification = classify_command(segment)
        segment_classifications.append(classification)

        blocked_match = starts_with_prefix(segment, blocked_prefixes)
        if blocked_match:
            mark(
                "deny",
                f"Blocked destructive command prefix: {blocked_match}",
                f"blocked_command:{blocked_match}",
            )
            continue

        if scope == "writes_and_high_risk_only":
            if classification == "read":
                matched_rules.append("scope:writes_and_high_risk_only")
            elif classification == "unknown":
                if unknown_mode == "allow_warn":
                    mark(
                        "warn",
                        "Unknown command classification allowed with warning",
                        "unknown_command:allow_warn",
                    )
                elif unknown_mode == "allow_silent":
                    matched_rules.append("unknown_command:allow_silent")
                else:
                    mark(
                        "deny",
                        "Unknown command blocked by policy",
                        f"unknown_command:{unknown_mode}",
                    )

        if classification in {"write", "high_risk"}:
            blocked, detail = matches_blocked_path(segment, blocked_paths)
            if blocked:
                mark(
                    "deny",
                    f"Write to blocked path detected ({detail})",
                    "blocked_path",
                )

    if not segments and raw:
        blocked_match = raw_contains_prefix(raw, blocked_prefixes)
        if blocked_match:
            mark(
                "deny",
                f"Blocked destructive command prefix: {blocked_match}",
                f"blocked_command:{blocked_match}",
            )

    diff_reason, diff_rules, diff_meta = _evaluate_diff_budget(event, segment_classifications, policy)
    matched_rules.extend(diff_rules)
    if diff_reason:
        mark("deny", diff_reason, "diff_budget:exceeded")

    if not matched_rules:
        matched_rules.append("default_allow")

    return Decision(
        decision=current_decision,
        reason=current_reason,
        matched_rules=_dedupe(matched_rules),
        classification=_overall_classification(segment_classifications, command),
        command=command,
        metadata={
            "segments": segments,
            "scope": scope,
            "unknown_command_mode": unknown_mode,
            **diff_meta,
        },
    )


def run_event(event: str, platform: str = DEFAULT_PLATFORM) -> int:
    started = time.perf_counter()
    payload = read_stdin_payload()
    policy = load_policy()
    decision = decide_event(event, payload, policy)
    latency_ms = int((time.perf_counter() - started) * 1000)
    log_decision(event, decision, policy, payload, platform, latency_ms)

    if decision.decision == "deny":
        print(decision.reason)
        return 2
    if decision.decision == "warn":
        print(decision.reason, file=sys.stderr)
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
