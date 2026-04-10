from __future__ import annotations

import datetime as dt
import json
from pathlib import Path
from typing import Any

from hook_types import Decision
from policy_io import ROOT


def _first(payload: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = payload.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def log_decision(
    event: str,
    decision: Decision,
    policy: dict[str, Any],
    payload: dict[str, Any],
    platform: str,
    latency_ms: int,
) -> None:
    logging_cfg = policy.get("logging", {})
    if logging_cfg.get("enabled", True) is False:
        return

    log_dir = logging_cfg.get("dir", ".harness/logs")
    out_dir = (ROOT / log_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d")
    log_path = out_dir / f"{stamp}.jsonl"
    record = {
        "timestamp": dt.datetime.now(dt.timezone.utc).isoformat(),
        "policy_version": policy.get("policy", {}).get("version", 1),
        "platform": platform or "unknown",
        "event": event,
        "event_id": _first(payload, "event_id", "id", "tool_call_id"),
        "session_id": _first(payload, "session_id", "conversation_id", "thread_id"),
        "decision_latency_ms": latency_ms,
        "command": decision.command,
        "classification": decision.classification,
        "decision": decision.decision,
        "reason": decision.reason,
        "matched_rules": decision.matched_rules,
        "metadata": decision.metadata,
    }
    with log_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, ensure_ascii=True) + "\n")
