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


def split_command_segments(command: str) -> list[str]:
    """
    Split shell text into independently evaluated segments.
    Separators: &&, ||, ;, | (outside quotes).
    """
    if not command.strip():
        return []

    out: list[str] = []
    token: list[str] = []
    quote: str | None = None
    escape = False
    index = 0
    text = command

    def flush() -> None:
        segment = "".join(token).strip()
        token.clear()
        if segment:
            out.append(segment)

    while index < len(text):
        ch = text[index]
        nxt = text[index + 1] if index + 1 < len(text) else ""

        if escape:
            token.append(ch)
            escape = False
            index += 1
            continue

        if ch == "\\" and quote != "'":
            token.append(ch)
            escape = True
            index += 1
            continue

        if quote:
            token.append(ch)
            if ch == quote:
                quote = None
            index += 1
            continue

        if ch in {"'", '"'}:
            quote = ch
            token.append(ch)
            index += 1
            continue

        if ch == "&" and nxt == "&":
            flush()
            index += 2
            continue
        if ch == "|" and nxt == "|":
            flush()
            index += 2
            continue
        if ch in {";", "|", "\n"}:
            flush()
            index += 1
            continue

        token.append(ch)
        index += 1

    flush()
    return out


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


def classify_command(
    command: str,
    *,
    allowed_prefixes: list[str],
    write_prefixes: list[str],
    high_risk_prefixes: list[str],
) -> str:
    """
    Return one of: read | write | high_risk | unknown
    """
    normalized = command.strip()
    if not normalized:
        return "unknown"

    low = normalized.lower()
    if any(
        low == prefix.lower() or low.startswith(prefix.lower() + " ")
        for prefix in high_risk_prefixes
        if prefix.strip()
    ):
        return "high_risk"

    if any(
        low == prefix.lower() or low.startswith(prefix.lower() + " ")
        for prefix in allowed_prefixes
        if prefix.strip()
    ):
        return "read"

    if any(
        low == prefix.lower() or low.startswith(prefix.lower() + " ")
        for prefix in write_prefixes
        if prefix.strip()
    ):
        return "write"

    if any(token in normalized for token in (">", ">>")):
        return "write"

    return "unknown"
