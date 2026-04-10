#!/usr/bin/env python3
from __future__ import annotations

from policy_hook import run_event


def main() -> int:
    return run_event("PostToolUse")


if __name__ == "__main__":
    raise SystemExit(main())
