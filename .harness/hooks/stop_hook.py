#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


DEFAULT_EVENT = "Stop"

def run_command(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )
    except FileNotFoundError:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"missing_executable:{command[0]}",
        )
    except OSError as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"os_error:{exc}",
        )


def _print_subprocess_error(result: subprocess.CompletedProcess[str], code: str) -> None:
    detail = (result.stderr or result.stdout or "").strip().replace("\n", " ")
    if detail:
        print(f"DENY {code}:{detail}")
    else:
        print(f"DENY {code}")


def has_changed_rust_files(cwd: Path) -> bool:
    inside_repo = run_command(["git", "rev-parse", "--is-inside-work-tree"], cwd)
    if inside_repo.returncode != 0:
        return False

    status = run_command(
        ["git", "status", "--porcelain", "--untracked-files=all", "--", "*.rs"],
        cwd,
    )
    if status.returncode != 0:
        return False

    for line in status.stdout.splitlines():
        entry = line.strip()
        if not entry:
            continue
        if len(entry) > 3:
            path_part = entry[3:]
        else:
            path_part = entry
        if " -> " in path_part:
            path_part = path_part.split(" -> ", 1)[1]
        candidate = path_part.strip().strip('"')
        if candidate.endswith(".rs"):
            return True
    return False


def run_stop() -> int:
    cwd = Path.cwd()
    if not has_changed_rust_files(cwd):
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
