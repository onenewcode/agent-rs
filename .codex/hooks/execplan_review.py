#!/usr/bin/env python3

from __future__ import annotations

import re
import subprocess
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

    plans = [
        path
        for path in active_dir.glob("*.md")
        if path.name != "TEMPLATE.md"
    ]
    if not plans:
        return None

    dirty_paths = get_dirty_exec_plans(repo_root)
    if dirty_paths:
        dirty_plans = [path for path in plans if path in dirty_paths]
        if dirty_plans:
            return max(dirty_plans, key=lambda path: (path.stat().st_mtime, path.name))

    return None


def get_dirty_exec_plans(repo_root: Path) -> set[Path]:
    result = subprocess.run(
        ["git", "status", "--porcelain", "--", "docs/exec-plans/active"],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )

    dirty_paths: set[Path] = set()
    for raw_line in result.stdout.splitlines():
        if len(raw_line) < 4:
            continue
        rel_path = raw_line[3:].strip()
        if not rel_path:
            continue

        path = repo_root / rel_path
        if path.name == "TEMPLATE.md":
            continue
        dirty_paths.add(path)

    return dirty_paths


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
