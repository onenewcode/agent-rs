#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from typing import Any


HARNESS_DIR = Path(__file__).resolve().parent
REPO_ROOT = HARNESS_DIR.parent
SUPPORTED_TARGETS = ("codex", "claude", "gemini")


def _load_policy_hook_module():
    module_path = HARNESS_DIR / "hooks" / "common.py"
    spec = importlib.util.spec_from_file_location("hook_common", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load policy hook module from {module_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


policy_hook = _load_policy_hook_module()


def ensure_directories(repo_root: Path, targets: list[str]) -> None:
    required = [
        ".harness/hooks",
        ".harness/logs",
        ".harness/templates",
    ]
    if "codex" in targets:
        required.extend((".codex", ".codex/rules"))
    if "claude" in targets:
        required.append(".claude")
    if "gemini" in targets:
        required.append(".gemini")

    for relative in required:
        (repo_root / relative).mkdir(parents=True, exist_ok=True)


def load_template(name: str) -> str:
    path = HARNESS_DIR / "templates" / name
    return path.read_text(encoding="utf-8")


def markdown_list(items: list[str]) -> str:
    return "\n".join(f"- `{item}`" for item in items)


def render_instruction(title: str, task_body: str, policy: dict[str, Any], platform_notes: str) -> str:
    template = load_template("instruction.md.tmpl")
    return template.format(
        title=title,
        task_body=task_body.strip(),
        allowed_paths=markdown_list(policy["allowed_paths"]),
        blocked_paths=markdown_list(policy["blocked_paths"]),
        allowed_commands=markdown_list(policy["allowed_commands"]),
        blocked_commands=markdown_list(policy["blocked_commands"]),
        max_files_changed=policy["max_files_changed"],
        max_lines_added=policy["max_lines_added"],
        max_lines_deleted=policy["max_lines_deleted"],
        platform_notes=platform_notes.strip(),
    ).rstrip() + "\n"


def render_commands_doc(policy: dict[str, Any]) -> str:
    template = load_template("commands.md.tmpl")
    return template.format(
        allowed_commands=markdown_list(policy["allowed_commands"]),
        blocked_commands=markdown_list(policy["blocked_commands"]),
    ).rstrip() + "\n"


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_json(path: Path, payload: dict[str, Any]) -> None:
    write_text(path, json.dumps(payload, indent=2, ensure_ascii=True, sort_keys=True) + "\n")


def render_codex_files(repo_root: Path, policy: dict[str, Any], task_body: str) -> None:
    write_text(
        repo_root / "AGENTS.md",
        render_instruction(
            "Harness Instructions For Codex",
            task_body,
            policy,
            (
                "Codex uses `AGENTS.md` for task instructions. Command policy is enforced through "
                "`.codex/hooks.json`, and Codex hook interception currently applies to `Bash` tool "
                "events in `PreToolUse` and `PostToolUse`."
            ),
        ),
    )

    write_text(
        repo_root / ".codex" / "config.toml",
        '[features]\n'
        'codex_hooks = true\n',
    )

    pre_tool_command = '/usr/bin/env python3 "$(git rev-parse --show-toplevel)/.harness/hooks/pre_tool.py"'
    post_tool_command = '/usr/bin/env python3 "$(git rev-parse --show-toplevel)/.harness/hooks/post_tool.py"'
    stop_command = '/usr/bin/env python3 "$(git rev-parse --show-toplevel)/.harness/hooks/stop.py"'
    write_json(
        repo_root / ".codex" / "hooks.json",
        {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": f'HARNESS_PLATFORM=codex {pre_tool_command}',
                            }
                        ],
                    }
                ],
                "PostToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": f'HARNESS_PLATFORM=codex {post_tool_command}',
                            }
                        ],
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": f'HARNESS_PLATFORM=codex {stop_command}',
                            }
                        ]
                    }
                ],
            }
        },
    )

    write_text(
        repo_root / ".codex" / "rules" / "commands.md",
        render_commands_doc(policy),
    )


def render_claude_files(repo_root: Path, policy: dict[str, Any], task_body: str) -> None:
    write_text(
        repo_root / "CLAUDE.md",
        render_instruction(
            "Harness Instructions For Claude",
            task_body,
            policy,
            (
                "Claude uses `CLAUDE.md` for project guidance. The generated settings attach "
                "shared hooks to `PreToolUse`, `PostToolUse`, and `Stop`. Shell commands are "
                "checked before execution; path and diff budgets are checked after tool use and at stop."
            ),
        ),
    )

    pre_tool_command = 'HARNESS_PLATFORM=claude python3 "$CLAUDE_PROJECT_DIR/.harness/hooks/pre_tool.py"'
    post_tool_command = 'HARNESS_PLATFORM=claude python3 "$CLAUDE_PROJECT_DIR/.harness/hooks/post_tool.py"'
    stop_command = 'HARNESS_PLATFORM=claude python3 "$CLAUDE_PROJECT_DIR/.harness/hooks/stop.py"'
    write_json(
        repo_root / ".claude" / "settings.json",
        {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": pre_tool_command,
                            }
                        ],
                    }
                ],
                "PostToolUse": [
                    {
                        "matcher": "Bash|Write|Edit|MultiEdit|NotebookEdit",
                        "hooks": [
                            {
                                "type": "command",
                                "command": post_tool_command,
                            }
                        ],
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": stop_command,
                            }
                        ]
                    }
                ],
            }
        },
    )


def render_gemini_files(repo_root: Path, policy: dict[str, Any], task_body: str) -> None:
    write_text(
        repo_root / "GEMINI.md",
        render_instruction(
            "Harness Instructions For Gemini",
            task_body,
            policy,
            (
                "Gemini uses `GEMINI.md` for project guidance. The generated settings attach shared "
                "hooks to `BeforeTool`, `AfterTool`, and `AfterAgent`. The hook enforces shell "
                "commands before execution and validates changed paths plus diff budgets afterwards."
            ),
        ),
    )

    pre_tool_command = 'HARNESS_PLATFORM=gemini python3 "$GEMINI_PROJECT_DIR/.harness/hooks/pre_tool.py"'
    post_tool_command = 'HARNESS_PLATFORM=gemini python3 "$GEMINI_PROJECT_DIR/.harness/hooks/post_tool.py"'
    stop_command = 'HARNESS_PLATFORM=gemini python3 "$GEMINI_PROJECT_DIR/.harness/hooks/stop.py"'
    write_json(
        repo_root / ".gemini" / "settings.json",
        {
            "hooks": {
                "BeforeTool": [
                    {
                        "matcher": "run_shell_command",
                        "hooks": [
                            {
                                "type": "command",
                                "command": pre_tool_command,
                            }
                        ],
                    }
                ],
                "AfterTool": [
                    {
                        "matcher": "run_shell_command|write_file|replace",
                        "hooks": [
                            {
                                "type": "command",
                                "command": post_tool_command,
                            }
                        ],
                    }
                ],
                "AfterAgent": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": stop_command,
                            }
                        ]
                    }
                ],
            }
        },
    )


def outputs_for_targets(repo_root: Path, targets: list[str]) -> list[Path]:
    outputs: list[Path] = []
    if "codex" in targets:
        outputs.extend(
            [
                repo_root / "AGENTS.md",
                repo_root / ".codex" / "config.toml",
                repo_root / ".codex" / "hooks.json",
                repo_root / ".codex" / "rules" / "commands.md",
            ]
        )
    if "claude" in targets:
        outputs.extend(
            [
                repo_root / "CLAUDE.md",
                repo_root / ".claude" / "settings.json",
            ]
        )
    if "gemini" in targets:
        outputs.extend(
            [
                repo_root / "GEMINI.md",
                repo_root / ".gemini" / "settings.json",
            ]
        )
    return outputs


def validate_outputs(repo_root: Path, targets: list[str]) -> None:
    expected = outputs_for_targets(repo_root, targets)
    missing = [str(path.relative_to(repo_root)) for path in expected if not path.exists()]
    if missing:
        raise RuntimeError(f"Missing generated outputs: {', '.join(missing)}")


def parse_targets(args: list[str]) -> list[str]:
    if not args:
        raise ValueError("usage: python3 .harness/sync.py <codex|claude|gemini|all> [target...]")

    requested: list[str] = []
    for arg in args:
        if arg == "all":
            requested.extend(SUPPORTED_TARGETS)
            continue
        if arg not in SUPPORTED_TARGETS:
            raise ValueError(f"unsupported target '{arg}'")
        requested.append(arg)

    ordered: list[str] = []
    for target in requested:
        if target not in ordered:
            ordered.append(target)
    return ordered


def sync_repo(targets: list[str], repo_root: Path | None = None) -> list[Path]:
    root = repo_root.resolve() if repo_root is not None else REPO_ROOT
    ensure_directories(root, targets)
    policy = policy_hook.load_policy(root)
    task_path = root / policy["task_file"]
    task_body = task_path.read_text(encoding="utf-8")

    if "codex" in targets:
        render_codex_files(root, policy, task_body)
    if "claude" in targets:
        render_claude_files(root, policy, task_body)
    if "gemini" in targets:
        render_gemini_files(root, policy, task_body)
    validate_outputs(root, targets)

    return outputs_for_targets(root, targets)


def main() -> int:
    try:
        targets = parse_targets(sys.argv[1:])
        generated = sync_repo(targets)
    except Exception as exc:
        print(f"sync failed: {exc}", file=sys.stderr)
        return 1

    print(f"Generated {len(generated)} files.")
    for path in generated:
        print(path.relative_to(REPO_ROOT).as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
