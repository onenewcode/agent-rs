#!/usr/bin/env python3
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


DEFAULT_EVENT = "Stop"
IGNORED_DIRS = {".git", "target", "node_modules", ".venv", "venv"}


def has_rust_files(root: Path) -> bool:
    for current_root, dirnames, filenames in os.walk(root):
        dirnames[:] = [name for name in dirnames if name not in IGNORED_DIRS]
        if any(name.endswith(".rs") for name in filenames):
            return True
    return False


def run_command(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )


def _print_subprocess_error(result: subprocess.CompletedProcess[str], code: str) -> None:
    detail = (result.stderr or result.stdout or "").strip().replace("\n", " ")
    if detail:
        print(f"DENY {code}:{detail}")
    else:
        print(f"DENY {code}")


def run_stop() -> int:
    cwd = Path.cwd()
    if not has_rust_files(cwd):
        return 0

    manifest = cwd / "Cargo.toml"
    if not manifest.exists():
        print("WARN stop_no_cargo_manifest", file=sys.stderr)
        return 0

    fmt = run_command(["cargo", "fmt", "--all"], cwd)
    if fmt.returncode != 0:
        _print_subprocess_error(fmt, "stop_fmt_failed")
        return 2

    clippy = run_command(
        [
            "cargo",
            "clippy",
            "--fix",
            "--allow-dirty",
            "--allow-staged",
            "--all-targets",
            "--all-features",
        ],
        cwd,
    )
    if clippy.returncode != 0:
        _print_subprocess_error(clippy, "stop_clippy_fix_failed")
        return 2

    return 0


def main() -> int:
    _ = sys.stdin.read()
    event = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_EVENT
    if event != "Stop":
        return 0
    return run_stop()


if __name__ == "__main__":
    raise SystemExit(main())
