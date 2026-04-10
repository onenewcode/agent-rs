#!/usr/bin/env python3
from __future__ import annotations

import datetime as dt
import json
import os
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib  # py311+
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None  # type: ignore[assignment]


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / ".harness" / "policy.toml"


@dataclass
class Decision:
    decision: str  # allow | deny | warn
    reason: str
    matched_rules: list[str]
    classification: str
    command: str


def _load_policy() -> dict[str, Any]:
    if tomllib is None:
        return {}
    if not POLICY_PATH.exists():
        return {}
    return tomllib.loads(POLICY_PATH.read_text(encoding="utf-8"))


def _read_stdin_payload() -> dict[str, Any]:
    raw = sys.stdin.read().strip()
    if not raw:
        return {"_raw_stdin": ""}
    try:
        obj = json.loads(raw)
        if isinstance(obj, dict):
            obj["_raw_stdin"] = raw
            return obj
    except json.JSONDecodeError:
        pass
    return {"_raw_stdin": raw}


def _extract_command_from_raw(raw: str) -> str:
    if not raw:
        return ""
    patterns = [
        r'"command"\s*:\s*"([^"]+)"',
        r'"cmd"\s*:\s*"([^"]+)"',
        r'"bash_command"\s*:\s*"([^"]+)"',
    ]
    for pat in patterns:
        m = re.search(pat, raw)
        if m:
            return m.group(1).encode("utf-8").decode("unicode_escape").strip()
    return ""


def _scan_for_command(payload: dict[str, Any]) -> str:
    # Strict key-based extraction only; avoid generic recursion that can pick UUIDs.
    direct = ("command", "cmd", "bash_command")
    for key in direct:
        val = payload.get(key)
        if isinstance(val, str) and val.strip():
            return val.strip()

    for container_key in ("tool_input", "input", "arguments", "params"):
        sub = payload.get(container_key)
        if isinstance(sub, dict):
            for key in direct:
                val = sub.get(key)
                if isinstance(val, str) and val.strip():
                    return val.strip()
        if isinstance(sub, str) and sub.strip():
            # Only accept obvious shell-like strings.
            text = sub.strip()
            if " " in text or text.startswith(("git", "ls", "cat", "rg", "rm", "python3")):
                return text

    # Last resort: parse the raw stdin JSON text for known keys.
    raw = payload.get("_raw_stdin", "")
    if isinstance(raw, str):
        return _extract_command_from_raw(raw)
    return ""


def _normalize_cmd_prefix(command: str) -> str:
    try:
        parts = shlex.split(command)
    except ValueError:
        parts = command.split()
    if not parts:
        return ""
    return " ".join(parts[:2]).strip().lower()


def _starts_with_prefix(command: str, prefixes: list[str]) -> str | None:
    cmd = command.strip().lower()
    for pref in prefixes:
        p = pref.strip().lower()
        if not p:
            continue
        if cmd == p or cmd.startswith(p + " "):
            return pref
    return None


def _raw_contains_blocked(raw: str, prefixes: list[str]) -> str | None:
    low = raw.lower()
    for pref in prefixes:
        p = pref.strip().lower()
        if not p:
            continue
        # Match either plain text or JSON-escaped text containing the command prefix.
        if f'"{p}' in low or f" {p} " in low or f"'{p}" in low:
            return pref
    return None


def _classify_command(command: str) -> str:
    """
    Return one of: read | write | high_risk | unknown
    """
    cmd = command.strip()
    if not cmd:
        return "unknown"

    low = cmd.lower()
    if any(
        low == p or low.startswith(p + " ")
        for p in ("rm", "git reset", "git checkout", "git clean")
    ):
        return "high_risk"

    read_prefixes = (
        "git status",
        "git diff",
        "git ls-files",
        "ls",
        "cat",
        "rg",
        "pwd",
        "which",
        "echo",
        "head",
        "tail",
        "wc",
    )
    if any(low == p or low.startswith(p + " ") for p in read_prefixes):
        return "read"

    write_prefixes = (
        "git add",
        "git commit",
        "git mv",
        "git rm",
        "git apply",
        "touch",
        "mkdir",
        "cp",
        "mv",
        "tee",
        "truncate",
        "chmod",
        "chown",
        "ln",
    )
    if any(low == p or low.startswith(p + " ") for p in write_prefixes):
        return "write"

    # Conservative write-signal operators.
    if any(token in cmd for token in (">", ">>")):
        return "write"

    return "unknown"


def _matches_blocked_path(command: str, blocked_globs: list[str]) -> tuple[bool, str]:
    if not blocked_globs:
        return False, ""
    # Minimal path extraction heuristics from tokens and redirection targets.
    candidates: list[str] = []
    try:
        tokens = shlex.split(command)
    except ValueError:
        tokens = command.split()
    for tok in tokens:
        if tok in {">", ">>", "2>", "1>", "2>>", "1>>"}:
            continue
        if tok.startswith("-"):
            continue
        if "/" in tok or tok.startswith("."):
            candidates.append(tok)

    candidates.extend(re.findall(r"(?:>>?|2>>?|1>>?)\s*([^\s]+)", command))

    for raw in candidates:
        path_text = raw.strip().strip("\"'")
        if not path_text:
            continue
        norm = path_text.replace("\\", "/")
        for pat in blocked_globs:
            p = pat.replace("\\", "/")
            if p == ".git/**":
                if norm == ".git" or norm.startswith(".git/") or "/.git/" in norm:
                    return True, f"path:{path_text}"
            else:
                # Very small fallback: prefix-like match for "*.*/**" styles.
                anchor = p.removesuffix("/**")
                if anchor and (norm == anchor or norm.startswith(anchor + "/")):
                    return True, f"path:{path_text}"
    return False, ""


def _log_decision(event: str, d: Decision) -> None:
    policy = _load_policy()
    logging_cfg = policy.get("logging", {})
    if logging_cfg.get("enabled", True) is False:
        return
    log_dir = logging_cfg.get("dir", ".harness/logs")
    out_dir = (ROOT / log_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d")
    out_file = out_dir / f"{stamp}.jsonl"
    record = {
        "timestamp": dt.datetime.now(dt.timezone.utc).isoformat(),
        "event": event,
        "command": d.command,
        "classification": d.classification,
        "decision": d.decision,
        "reason": d.reason,
        "matched_rules": d.matched_rules,
    }
    with out_file.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(record, ensure_ascii=True) + "\n")


def decide(event: str, payload: dict[str, Any]) -> Decision:
    policy = _load_policy()
    commands = policy.get("commands", {})
    paths = policy.get("paths", {})
    enforcement = policy.get("enforcement", {})

    command = _scan_for_command(payload)
    raw = payload.get("_raw_stdin", "")
    if not isinstance(raw, str):
        raw = ""
    classification = _classify_command(command)

    blocked_prefixes = list(commands.get("blocked_prefixes", []))
    blocked_match = _starts_with_prefix(command, blocked_prefixes) if command else None
    if blocked_match is None and raw:
        blocked_match = _raw_contains_blocked(raw, blocked_prefixes)
    if blocked_match:
        return Decision(
            decision="deny",
            reason=f"Blocked destructive command prefix: {blocked_match}",
            matched_rules=[f"blocked_command:{blocked_match}"],
            classification="high_risk",
            command=command,
        )

    # Scope: only evaluate write/high-risk commands.
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

    blocked_paths = list(paths.get("blocked_write_paths", [])) or list(
        paths.get("blocked_paths", [])
    )
    is_write = classification in {"write", "high_risk"}
    if is_write:
        hit, detail = _matches_blocked_path(command, blocked_paths)
        if hit:
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
    event = sys.argv[1] if len(sys.argv) > 1 else "Unknown"
    payload = _read_stdin_payload()
    decision = decide(event, payload)
    _log_decision(event, decision)

    if decision.decision == "deny":
        print(decision.reason)
        return 2

    if decision.decision == "warn":
        # Non-blocking warning for unknown commands in relaxed mode.
        print(decision.reason, file=sys.stderr)
        return 0

    return 0


if __name__ == "__main__":
    sys.exit(main())
