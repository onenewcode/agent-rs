from __future__ import annotations

import re
import shlex
from pathlib import PurePosixPath


def matches_blocked_path(command: str, blocked_globs: list[str]) -> tuple[bool, str]:
    if not blocked_globs:
        return False, ""

    candidates: list[str] = []
    try:
        tokens = shlex.split(command)
    except ValueError:
        tokens = command.split()

    for token in tokens:
        if token in {">", ">>", "2>", "1>", "2>>", "1>>"}:
            continue
        if token.startswith("-"):
            continue
        if "/" in token or token.startswith("."):
            candidates.append(token)

    candidates.extend(re.findall(r"(?:>>?|2>>?|1>>?)\s*([^\s]+)", command))

    for raw in candidates:
        path_text = raw.strip().strip("\"'")
        if not path_text:
            continue
        normalized = path_text.replace("\\", "/")

        parts = PurePosixPath(normalized).parts
        if ".." in parts:
            return True, f"path_traversal:{path_text}"

        for pattern in blocked_globs:
            blocked = pattern.replace("\\", "/")
            if blocked == ".git/**":
                if (
                    normalized == ".git"
                    or normalized.startswith(".git/")
                    or "/.git/" in normalized
                ):
                    return True, f"path:{path_text}"
                continue

            anchor = blocked.removesuffix("/**")
            if anchor and (normalized == anchor or normalized.startswith(anchor + "/")):
                return True, f"path:{path_text}"

    return False, ""
