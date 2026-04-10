from __future__ import annotations

from dataclasses import dataclass


@dataclass
class Decision:
    decision: str  # allow | deny | warn
    reason: str
    matched_rules: list[str]
    classification: str
    command: str
