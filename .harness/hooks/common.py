from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
import traceback
from fnmatch import fnmatchcase
from pathlib import Path
from typing import Any

import tomllib


DEFAULT_POLICY: dict[str, Any] = {
    "allowed_paths": [
        ".harness/**",
        ".codex/**",
        ".claude/**",
        ".gemini/**",
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
    ],
    "blocked_paths": [
        ".git/**",
    ],
    "allowed_commands": [
        "git status",
        "git diff",
        "git ls-files",
        "ls",
        "cat",
        "rg",
    ],
    "blocked_commands": [
        "rm",
        "git reset",
        "git checkout",
        "git clean",
    ],
    "max_files_changed": 20,
    "max_lines_added": 500,
    "max_lines_deleted": 200,
    "task_file": ".harness/task.md",
}

INTERNAL_IGNORED_PATHS = (".harness/logs/**",)
PAYLOAD_ENV_KEYS = (
    "HARNESS_HOOK_PAYLOAD",
    "HOOK_PAYLOAD",
    "CODEX_HOOK_PAYLOAD",
    "CLAUDE_HOOK_PAYLOAD",
    "GEMINI_HOOK_PAYLOAD",
)


def repo_root_from_env() -> Path | None:
    for key in ("HARNESS_REPO_ROOT", "CLAUDE_PROJECT_DIR", "GEMINI_PROJECT_DIR"):
        value = os.environ.get(key)
        if value:
            return Path(value).expanduser().resolve()
    return None


def resolve_repo_root(repo_root: str | Path | None = None) -> Path:
    if repo_root is not None:
        return Path(repo_root).expanduser().resolve()

    env_root = repo_root_from_env()
    if env_root is not None:
        return env_root

    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return Path(__file__).resolve().parents[2]

    return Path(result.stdout.strip()).resolve()


def _validate_string_list(value: Any, field_name: str) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ValueError(f"{field_name} must be a list of strings")
    return value


def _validate_int(value: Any, field_name: str) -> int:
    if not isinstance(value, int) or value < 0:
        raise ValueError(f"{field_name} must be a non-negative integer")
    return value


def normalize_policy(data: dict[str, Any] | None = None) -> dict[str, Any]:
    merged = dict(DEFAULT_POLICY)
    if data:
        merged.update(data)

    return {
        "allowed_paths": _validate_string_list(merged.get("allowed_paths"), "allowed_paths"),
        "blocked_paths": _validate_string_list(merged.get("blocked_paths"), "blocked_paths"),
        "allowed_commands": _validate_string_list(merged.get("allowed_commands"), "allowed_commands"),
        "blocked_commands": _validate_string_list(merged.get("blocked_commands"), "blocked_commands"),
        "max_files_changed": _validate_int(merged.get("max_files_changed"), "max_files_changed"),
        "max_lines_added": _validate_int(merged.get("max_lines_added"), "max_lines_added"),
        "max_lines_deleted": _validate_int(merged.get("max_lines_deleted"), "max_lines_deleted"),
        "task_file": str(merged.get("task_file") or DEFAULT_POLICY["task_file"]),
    }


def load_policy(repo_root: str | Path | None = None) -> dict[str, Any]:
    root = resolve_repo_root(repo_root)
    policy_path = root / ".harness" / "policy.toml"
    if not policy_path.exists():
        return normalize_policy()

    with policy_path.open("rb") as handle:
        raw = tomllib.load(handle)
    if raw and not isinstance(raw, dict):
        raise ValueError("policy.toml must contain a top-level table")
    return normalize_policy(raw)


def _split_prefix(prefix: str) -> list[str]:
    return shlex.split(prefix)


def _command_tokens(command: str | list[str] | None) -> list[str]:
    if command is None:
        return []
    if isinstance(command, list):
        return [str(part) for part in command]
    return shlex.split(command)


def _matches_prefix(tokens: list[str], prefix: str) -> bool:
    prefix_tokens = _split_prefix(prefix)
    if len(prefix_tokens) > len(tokens):
        return False
    return tokens[: len(prefix_tokens)] == prefix_tokens


def match_command(policy: dict[str, Any], command: str | list[str] | None) -> tuple[bool, str | None]:
    tokens = _command_tokens(command)
    if not tokens:
        return True, None

    for prefix in policy["blocked_commands"]:
        if _matches_prefix(tokens, prefix):
            return False, f"Command '{' '.join(tokens)}' is blocked by prefix '{prefix}'."

    for prefix in policy["allowed_commands"]:
        if _matches_prefix(tokens, prefix):
            return True, None

    return False, f"Command '{' '.join(tokens)}' is not in allowed_commands."


def _normalize_path_string(path: str | Path, repo_root: Path, base_dir: Path | None = None) -> str:
    candidate = Path(path).expanduser()
    if candidate.is_absolute():
        resolved = candidate.resolve()
    else:
        anchor = base_dir.resolve() if base_dir is not None else repo_root
        resolved = (anchor / candidate).resolve()

    try:
        relative = resolved.relative_to(repo_root)
    except ValueError:
        return resolved.as_posix()
    return relative.as_posix()


def _glob_match(path: str, pattern: str) -> bool:
    normalized_path = path.lstrip("./")
    normalized_pattern = pattern.lstrip("./")
    path_parts = [part for part in normalized_path.split("/") if part]
    pattern_parts = [part for part in normalized_pattern.split("/") if part]

    def matches(path_index: int, pattern_index: int) -> bool:
        if pattern_index == len(pattern_parts):
            return path_index == len(path_parts)

        part = pattern_parts[pattern_index]
        if part == "**":
            if pattern_index == len(pattern_parts) - 1:
                return True
            for next_index in range(path_index, len(path_parts) + 1):
                if matches(next_index, pattern_index + 1):
                    return True
            return False

        if path_index >= len(path_parts):
            return False

        if not fnmatchcase(path_parts[path_index], part):
            return False
        return matches(path_index + 1, pattern_index + 1)

    return matches(0, 0)


def _is_internal_ignored_path(path: str) -> bool:
    return any(_glob_match(path, pattern) for pattern in INTERNAL_IGNORED_PATHS)


def match_path(
    policy: dict[str, Any],
    path: str | Path,
    repo_root: str | Path | None = None,
    base_dir: str | Path | None = None,
) -> tuple[bool, str | None]:
    root = resolve_repo_root(repo_root)
    resolved_base_dir = Path(base_dir).expanduser().resolve() if base_dir is not None else None
    normalized = _normalize_path_string(path, root, resolved_base_dir)
    if _is_internal_ignored_path(normalized):
        return True, None

    for pattern in policy["blocked_paths"]:
        if _glob_match(normalized, pattern):
            return False, f"Path '{normalized}' is blocked by pattern '{pattern}'."

    for pattern in policy["allowed_paths"]:
        if _glob_match(normalized, pattern):
            return True, None

    return False, f"Path '{normalized}' is outside allowed_paths."


def _run_git(repo_root: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )


def _parse_numstat(stdout: str) -> tuple[int, int]:
    added = 0
    deleted = 0
    for line in stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        add_text, del_text = parts[0], parts[1]
        if add_text != "-":
            added += int(add_text)
        if del_text != "-":
            deleted += int(del_text)
    return added, deleted


def _count_file_lines(path: Path) -> int:
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as handle:
            return sum(1 for _ in handle)
    except OSError:
        return 0


def collect_git_diff(repo_root: str | Path | None = None) -> dict[str, Any]:
    root = resolve_repo_root(repo_root)
    status = _run_git(root, ["status", "--short", "--untracked-files=all"])
    if status.returncode != 0:
        return {
            "error": status.stderr.strip() or "Failed to inspect git status.",
            "files": [],
            "files_changed": 0,
            "lines_added": 0,
            "lines_deleted": 0,
        }

    files: set[str] = set()
    untracked: set[str] = set()

    for line in status.stdout.splitlines():
        if len(line) < 4:
            continue
        state = line[:2]
        path_text = line[3:]
        if " -> " in path_text:
            path_text = path_text.split(" -> ", 1)[1]
        normalized = Path(path_text).as_posix()
        if _is_internal_ignored_path(normalized):
            continue
        files.add(normalized)
        if state == "??":
            untracked.add(normalized)

    staged = _run_git(root, ["diff", "--numstat", "--cached", "--relative"])
    unstaged = _run_git(root, ["diff", "--numstat", "--relative"])
    if staged.returncode != 0 or unstaged.returncode != 0:
        return {
            "error": "Failed to inspect git diff.",
            "files": sorted(files),
            "files_changed": len(files),
            "lines_added": 0,
            "lines_deleted": 0,
        }

    lines_added, lines_deleted = _parse_numstat(staged.stdout)
    extra_added, extra_deleted = _parse_numstat(unstaged.stdout)
    lines_added += extra_added
    lines_deleted += extra_deleted

    for relative_path in untracked:
        lines_added += _count_file_lines(root / relative_path)

    return {
        "error": None,
        "files": sorted(files),
        "files_changed": len(files),
        "lines_added": lines_added,
        "lines_deleted": lines_deleted,
    }


def check_budget(policy: dict[str, Any], diff: dict[str, Any]) -> list[str]:
    if diff.get("error"):
        return [str(diff["error"])]

    reasons: list[str] = []
    if diff["files_changed"] > policy["max_files_changed"]:
        reasons.append(
            f"Changed files {diff['files_changed']} exceed max_files_changed {policy['max_files_changed']}."
        )
    if diff["lines_added"] > policy["max_lines_added"]:
        reasons.append(
            f"Added lines {diff['lines_added']} exceed max_lines_added {policy['max_lines_added']}."
        )
    if diff["lines_deleted"] > policy["max_lines_deleted"]:
        reasons.append(
            f"Deleted lines {diff['lines_deleted']} exceed max_lines_deleted {policy['max_lines_deleted']}."
        )
    return reasons


def _payload_text() -> str:
    stdin_text = sys.stdin.read().strip()
    if stdin_text:
        return stdin_text
    for key in PAYLOAD_ENV_KEYS:
        value = os.environ.get(key)
        if value:
            return value
    raise ValueError("Hook payload was not provided on stdin or known environment variables.")


def load_payload() -> dict[str, Any]:
    payload = json.loads(_payload_text())
    if not isinstance(payload, dict):
        raise ValueError("Hook payload must be a JSON object.")
    return payload


def detect_platform(payload: dict[str, Any]) -> str:
    override = os.environ.get("HARNESS_PLATFORM")
    if override in {"codex", "claude", "gemini"}:
        return override

    event_name = payload.get("hook_event_name") or payload.get("event")
    if event_name in {"BeforeTool", "AfterTool", "AfterAgent"}:
        return "gemini"
    if payload.get("permission_mode") is not None:
        return "claude"
    if payload.get("transcript_path") or payload.get("cwd"):
        return "codex"
    raise ValueError("Unable to determine hook platform.")


def normalize_event(platform: str, payload: dict[str, Any]) -> str:
    event_name = payload.get("hook_event_name") or payload.get("event")
    mapping = {
        ("codex", "PreToolUse"): "pre_tool",
        ("codex", "PostToolUse"): "post_tool",
        ("codex", "Stop"): "stop",
        ("claude", "PreToolUse"): "pre_tool",
        ("claude", "PostToolUse"): "post_tool",
        ("claude", "Stop"): "stop",
        ("gemini", "BeforeTool"): "pre_tool",
        ("gemini", "AfterTool"): "post_tool",
        ("gemini", "AfterAgent"): "stop",
    }
    try:
        return mapping[(platform, event_name)]
    except KeyError as exc:
        raise ValueError(f"Unsupported hook event '{event_name}' for platform '{platform}'.") from exc


def _tool_name(payload: dict[str, Any]) -> str:
    return str(payload.get("tool_name") or payload.get("toolName") or "")


def _tool_input(payload: dict[str, Any]) -> dict[str, Any]:
    tool_input = payload.get("tool_input") or payload.get("toolInput") or {}
    return tool_input if isinstance(tool_input, dict) else {}


def extract_command(payload: dict[str, Any]) -> str | list[str] | None:
    tool_input = _tool_input(payload)
    for key in ("command", "cmd", "argv"):
        if key in tool_input:
            return tool_input[key]
    return None


def _unique_paths(values: list[str]) -> list[str]:
    seen: set[str] = set()
    ordered: list[str] = []
    for value in values:
        if value not in seen:
            seen.add(value)
            ordered.append(value)
    return ordered


def extract_paths(
    payload: dict[str, Any],
    repo_root: str | Path | None = None,
    base_dir: str | Path | None = None,
) -> list[str]:
    root = resolve_repo_root(repo_root)
    resolved_base_dir = Path(base_dir).expanduser().resolve() if base_dir is not None else root
    tool_input = _tool_input(payload)
    values: list[str] = []

    for key in ("file_path", "path", "target_file", "old_file_path", "new_file_path"):
        value = tool_input.get(key)
        if isinstance(value, str):
            values.append(_normalize_path_string(value, root, resolved_base_dir))

    for key in ("paths", "file_paths"):
        value = tool_input.get(key)
        if isinstance(value, list):
            for item in value:
                if isinstance(item, str):
                    values.append(_normalize_path_string(item, root, resolved_base_dir))

    return _unique_paths(values)


def _join_reasons(reasons: list[str]) -> str:
    return " ".join(reason.strip() for reason in reasons if reason.strip())


def emit_platform_response(
    platform: str,
    event: str,
    allowed: bool,
    reason: str | None = None,
    *,
    stop_hook_active: bool = False,
) -> dict[str, Any]:
    if allowed:
        return {}

    message = reason or "Policy check failed."
    if platform == "codex":
        if event == "pre_tool":
            return {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": message,
                }
            }
        if event == "post_tool":
            return {
                "decision": "block",
                "reason": message,
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": message,
                },
            }
        if stop_hook_active:
            return {
                "continue": False,
                "stopReason": message,
            }
        return {
            "decision": "block",
            "reason": message,
        }

    if platform == "claude":
        if event == "pre_tool":
            return {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": message,
                }
            }
        if stop_hook_active:
            return {
                "continue": False,
                "stopReason": message,
            }
        return {
            "decision": "block",
            "reason": message,
        }

    if platform == "gemini":
        if stop_hook_active:
            return {
                "continue": False,
                "stopReason": message,
            }
        return {
            "decision": "deny",
            "reason": message,
        }

    raise ValueError(f"Unsupported platform '{platform}'.")


def append_log(repo_root: str | Path | None, entry: dict[str, Any]) -> None:
    root = resolve_repo_root(repo_root)
    log_dir = root / ".harness" / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{entry['timestamp'][:10]}.jsonl"
    with log_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry, ensure_ascii=True, sort_keys=True) + "\n")


def evaluate_payload(
    payload: dict[str, Any],
    repo_root: str | Path | None = None,
    *,
    expected_event: str | None = None,
) -> dict[str, Any]:
    root = resolve_repo_root(repo_root)
    policy = load_policy(root)
    platform = detect_platform(payload)
    event = normalize_event(platform, payload)
    if expected_event is not None and event != expected_event:
        raise ValueError(f"Hook payload event '{event}' does not match entrypoint '{expected_event}'.")

    payload_cwd = payload.get("cwd")
    base_dir = payload_cwd if isinstance(payload_cwd, str) else root
    stop_hook_active = bool(payload.get("stop_hook_active"))
    tool_name = _tool_name(payload)
    command = extract_command(payload)
    diff = collect_git_diff(root) if event in {"post_tool", "stop"} else {
        "error": None,
        "files": [],
        "files_changed": 0,
        "lines_added": 0,
        "lines_deleted": 0,
    }

    reasons: list[str] = []

    if event == "pre_tool" and command is not None:
        allowed, reason = match_command(policy, command)
        if not allowed and reason is not None:
            reasons.append(reason)

    if event in {"post_tool", "stop"}:
        changed_paths = _unique_paths(extract_paths(payload, root, base_dir) + diff["files"])
        for path in changed_paths:
            allowed, reason = match_path(policy, path, root)
            if not allowed and reason is not None:
                reasons.append(reason)
        reasons.extend(check_budget(policy, diff))

    allowed = not reasons
    reason_text = _join_reasons(reasons) if reasons else None
    response = emit_platform_response(
        platform,
        event,
        allowed,
        reason_text,
        stop_hook_active=stop_hook_active,
    )

    return {
        "allowed": allowed,
        "command": command,
        "diff": diff,
        "event": event,
        "paths": _unique_paths(extract_paths(payload, root, base_dir) + diff["files"]),
        "platform": platform,
        "policy": policy,
        "reason": reason_text,
        "response": response,
        "repo_root": str(root),
        "stop_hook_active": stop_hook_active,
        "tool_name": tool_name,
    }


def _timestamp() -> str:
    from datetime import datetime, timezone

    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _build_log_entry(outcome: dict[str, Any]) -> dict[str, Any]:
    return {
        "timestamp": _timestamp(),
        "platform": outcome["platform"],
        "event": outcome["event"],
        "allowed": outcome["allowed"],
        "reason": outcome["reason"],
        "command": outcome["command"],
        "paths": outcome["paths"],
        "budget": {
            "files_changed": outcome["diff"]["files_changed"],
            "lines_added": outcome["diff"]["lines_added"],
            "lines_deleted": outcome["diff"]["lines_deleted"],
        },
    }


def _failure_response(
    payload: dict[str, Any] | None,
    error: Exception,
    *,
    expected_event: str | None = None,
) -> tuple[dict[str, Any], int]:
    message = f"Harness hook error: {error}"
    platform = "gemini"
    event = expected_event or "stop"
    stop_hook_active = False
    if payload is not None:
        try:
            platform = detect_platform(payload)
            stop_hook_active = bool(payload.get("stop_hook_active"))
            if expected_event is None:
                event = normalize_event(platform, payload)
        except Exception:
            pass
    response = emit_platform_response(
        platform,
        event,
        False,
        message,
        stop_hook_active=stop_hook_active,
    )
    return response, 0


def run_hook(expected_event: str) -> int:
    payload: dict[str, Any] | None = None
    try:
        payload = load_payload()
        outcome = evaluate_payload(payload, expected_event=expected_event)
        try:
            append_log(outcome["repo_root"], _build_log_entry(outcome))
        except OSError as exc:
            print(f"Harness log warning: {exc}", file=sys.stderr)

        if outcome["response"]:
            print(json.dumps(outcome["response"], ensure_ascii=True))
        return 0
    except Exception as exc:
        print("Harness hook failed.", file=sys.stderr)
        print("".join(traceback.format_exception_only(type(exc), exc)).strip(), file=sys.stderr)
        response, exit_code = _failure_response(payload, exc, expected_event=expected_event)
        if response:
            print(json.dumps(response, ensure_ascii=True))
        return exit_code
