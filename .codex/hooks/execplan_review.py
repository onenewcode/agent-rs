#!/usr/bin/env python3

from __future__ import annotations

import re
from pathlib import Path


FIELD_RE = re.compile(r"^- (?P<key>[^:]+):\s*(?P<value>.*)$")
RECOGNIZED_REVIEW_STATES = {
    "not_needed",
    "pending_human_review",
    "approved",
    "deferred",
}


def get_active_exec_plan(repo_root: Path) -> Path | None:
    active_dir = repo_root / "docs" / "exec-plans" / "active"
    if not active_dir.exists():
        return None

    plans = [path for path in active_dir.glob("*.md") if path.name != "TEMPLATE.md"]
    if not plans:
        return None

    return max(plans, key=lambda path: (path.stat().st_mtime, path.name))


def parse_execplan_review(plan_path: Path) -> dict[str, str]:
    fields: dict[str, str] = {}
    in_review_gate = False

    for raw_line in plan_path.read_text().splitlines():
        line = raw_line.rstrip()
        if line == "## Review Gate":
            in_review_gate = True
            continue
        if in_review_gate and line.startswith("## "):
            break
        if not in_review_gate:
            continue

        match = FIELD_RE.match(line)
        if match:
            fields[match.group("key").strip()] = match.group("value").strip()

    return fields
