from __future__ import annotations

import re
from typing import Any


def extract_command_from_raw(raw: str) -> str:
    if not raw:
        return ""
    patterns = [
        r'"command"\s*:\s*"([^"]+)"',
        r'"cmd"\s*:\s*"([^"]+)"',
        r'"bash_command"\s*:\s*"([^"]+)"',
    ]
    for pattern in patterns:
        match = re.search(pattern, raw)
        if match:
            return match.group(1).encode("utf-8").decode("unicode_escape").strip()
    return ""


def scan_for_command(payload: dict[str, Any]) -> str:
    direct_keys = ("command", "cmd", "bash_command")
    for key in direct_keys:
        value = payload.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()

    for container_key in ("tool_input", "input", "arguments", "params"):
        sub = payload.get(container_key)
        if isinstance(sub, dict):
            for key in direct_keys:
                value = sub.get(key)
                if isinstance(value, str) and value.strip():
                    return value.strip()
        if isinstance(sub, str) and sub.strip():
            text = sub.strip()
            if " " in text or text.startswith(("git", "ls", "cat", "rg", "rm", "python3")):
                return text

    raw = payload.get("_raw_stdin", "")
    if isinstance(raw, str):
        return extract_command_from_raw(raw)
    return ""


def starts_with_prefix(command: str, prefixes: list[str]) -> str | None:
    normalized = command.strip().lower()
    for prefix in prefixes:
        candidate = prefix.strip().lower()
        if not candidate:
            continue
        if normalized == candidate or normalized.startswith(candidate + " "):
            return prefix
    return None


def raw_contains_prefix(raw: str, prefixes: list[str]) -> str | None:
    lowered = raw.lower()
    for prefix in prefixes:
        candidate = prefix.strip().lower()
        if not candidate:
            continue
        if f'"{candidate}' in lowered or f" {candidate} " in lowered or f"'{candidate}" in lowered:
            return prefix
    return None


def classify_command(command: str) -> str:
    """
    Return one of: read | write | high_risk | unknown
    """
    normalized = command.strip()
    if not normalized:
        return "unknown"

    low = normalized.lower()
    if any(
        low == prefix or low.startswith(prefix + " ")
        for prefix in ("rm", "git reset", "git checkout", "git clean")
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
    if any(low == prefix or low.startswith(prefix + " ") for prefix in read_prefixes):
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
    if any(low == prefix or low.startswith(prefix + " ") for prefix in write_prefixes):
        return "write"

    if any(token in normalized for token in (">", ">>")):
        return "write"

    return "unknown"
