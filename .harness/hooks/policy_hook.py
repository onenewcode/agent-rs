#!/usr/bin/env python3
from __future__ import annotations

import sys

import post_tool
import pre_tool
import stop


def main() -> int:
    event = sys.argv[1] if len(sys.argv) > 1 else ""
    if event == "PreToolUse":
        return pre_tool.main()
    if event == "PostToolUse":
        return post_tool.main()
    if event == "Stop":
        return stop.main()

    print(f"Unsupported hook event: {event or 'Unknown'}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
