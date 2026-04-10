from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

try:
    import tomllib  # py311+
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None  # type: ignore[assignment]


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / ".harness" / "policy.toml"


def load_policy() -> dict[str, Any]:
    if tomllib is None:
        return {}
    if not POLICY_PATH.exists():
        return {}
    return tomllib.loads(POLICY_PATH.read_text(encoding="utf-8"))


def read_stdin_payload() -> dict[str, Any]:
    raw = sys.stdin.read().strip()
    if not raw:
        return {"_raw_stdin": ""}
    try:
        obj = json.loads(raw)
    except json.JSONDecodeError:
        return {"_raw_stdin": raw}
    if not isinstance(obj, dict):
        return {"_raw_stdin": raw}
    obj["_raw_stdin"] = raw
    return obj
