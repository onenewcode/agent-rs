#!/usr/bin/env python3
from __future__ import annotations

import sys

from policy_hook import main as policy_main

if __name__ == "__main__":
    sys.argv = [sys.argv[0], "Stop"]
    raise SystemExit(policy_main())
