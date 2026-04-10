from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class Decision:
    decision: str  # allow | deny | warn
    reason: str
    matched_rules: list[str]
    classification: str
    command: str
    metadata: dict[str, Any] = field(default_factory=dict)
