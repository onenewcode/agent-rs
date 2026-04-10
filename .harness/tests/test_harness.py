from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_DIR = REPO_ROOT / ".harness"


def load_module(path: Path, module_name: str):
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load module from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


hook_common = load_module(HARNESS_DIR / "hooks" / "common.py", "hook_common")
sync_module = load_module(HARNESS_DIR / "sync.py", "sync_module")


class HarnessTestCase(unittest.TestCase):
    def create_repo(self) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        repo_root = Path(temp_dir.name)
        subprocess.run(["git", "init"], cwd=repo_root, check=True, capture_output=True)

        for relative in (
            ".harness/hooks",
            ".harness/templates",
            ".harness/logs",
            ".harness/tests",
            ".codex/rules",
            ".claude",
            ".gemini",
        ):
            (repo_root / relative).mkdir(parents=True, exist_ok=True)

        copy_map = {
            HARNESS_DIR / "hooks" / "common.py": repo_root / ".harness" / "hooks" / "common.py",
            HARNESS_DIR / "hooks" / "pre_tool.py": repo_root / ".harness" / "hooks" / "pre_tool.py",
            HARNESS_DIR / "hooks" / "post_tool.py": repo_root / ".harness" / "hooks" / "post_tool.py",
            HARNESS_DIR / "hooks" / "stop.py": repo_root / ".harness" / "hooks" / "stop.py",
            HARNESS_DIR / "sync.py": repo_root / ".harness" / "sync.py",
            HARNESS_DIR / "templates" / "instruction.md.tmpl": repo_root / ".harness" / "templates" / "instruction.md.tmpl",
            HARNESS_DIR / "templates" / "commands.md.tmpl": repo_root / ".harness" / "templates" / "commands.md.tmpl",
        }
        for source, destination in copy_map.items():
            shutil.copy2(source, destination)

        (repo_root / ".harness" / "policy.toml").write_text(
            'task_file = ".harness/task.md"\n',
            encoding="utf-8",
        )
        (repo_root / ".harness" / "task.md").write_text(
            "# Temp Task\n\nImplement the harness.\n",
            encoding="utf-8",
        )
        return repo_root

    def git_commit(self, repo_root: Path, message: str = "baseline") -> None:
        subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=repo_root, check=True)
        subprocess.run(["git", "config", "user.name", "Harness Test"], cwd=repo_root, check=True)
        subprocess.run(["git", "add", "."], cwd=repo_root, check=True)
        subprocess.run(["git", "commit", "-m", message], cwd=repo_root, check=True, capture_output=True)

    def test_sync_generates_outputs_in_empty_repo(self) -> None:
        repo_root = self.create_repo()
        result = subprocess.run(
            [sys.executable, ".harness/sync.py", "codex"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("Generated 4 files.", result.stdout)
        for relative in (
            "AGENTS.md",
            ".codex/config.toml",
            ".codex/hooks.json",
            ".codex/rules/commands.md",
        ):
            self.assertTrue((repo_root / relative).exists(), relative)
        self.assertFalse((repo_root / "CLAUDE.md").exists())
        self.assertFalse((repo_root / "GEMINI.md").exists())
        self.assertFalse((repo_root / ".claude" / "settings.json").exists())
        self.assertFalse((repo_root / ".gemini" / "settings.json").exists())
        agents_text = (repo_root / "AGENTS.md").read_text(encoding="utf-8")
        self.assertIn("Harness Instructions For Codex", agents_text)
        self.assertIn("Implement the harness.", agents_text)
        self.assertNotIn("{title}", agents_text)
        commands_text = (repo_root / ".codex" / "rules" / "commands.md").read_text(encoding="utf-8")
        self.assertIn("- `git status`", commands_text)
        self.assertNotIn("{allowed_commands}", commands_text)
        codex_hooks = json.loads((repo_root / ".codex" / "hooks.json").read_text(encoding="utf-8"))
        self.assertIn("/.harness/hooks/pre_tool.py", codex_hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"])
        self.assertIn("/.harness/hooks/post_tool.py", codex_hooks["hooks"]["PostToolUse"][0]["hooks"][0]["command"])
        self.assertIn("/.harness/hooks/stop.py", codex_hooks["hooks"]["Stop"][0]["hooks"][0]["command"])

    def test_sync_is_idempotent(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py", "all"], cwd=repo_root, check=True)
        snapshot = {
            relative: (repo_root / relative).read_text(encoding="utf-8")
            for relative in (
                "AGENTS.md",
                "CLAUDE.md",
                "GEMINI.md",
                ".codex/config.toml",
                ".codex/hooks.json",
                ".codex/rules/commands.md",
                ".claude/settings.json",
                ".gemini/settings.json",
            )
        }
        subprocess.run([sys.executable, ".harness/sync.py", "all"], cwd=repo_root, check=True)
        current = {
            relative: (repo_root / relative).read_text(encoding="utf-8")
            for relative in snapshot
        }
        self.assertEqual(snapshot, current)

    def test_sync_requires_explicit_target(self) -> None:
        repo_root = self.create_repo()
        result = subprocess.run(
            [sys.executable, ".harness/sync.py"],
            cwd=repo_root,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("usage: python3 .harness/sync.py", result.stderr)

    def test_sync_all_generates_all_platform_outputs(self) -> None:
        repo_root = self.create_repo()
        result = subprocess.run(
            [sys.executable, ".harness/sync.py", "all"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("Generated 8 files.", result.stdout)
        for relative in (
            "AGENTS.md",
            "CLAUDE.md",
            "GEMINI.md",
            ".codex/config.toml",
            ".codex/hooks.json",
            ".codex/rules/commands.md",
            ".claude/settings.json",
            ".gemini/settings.json",
        ):
            self.assertTrue((repo_root / relative).exists(), relative)

    def test_load_policy_uses_defaults(self) -> None:
        repo_root = self.create_repo()
        policy = hook_common.load_policy(repo_root)
        self.assertEqual(policy["task_file"], ".harness/task.md")
        self.assertEqual(policy["max_files_changed"], 20)
        self.assertIn(".harness/**", policy["allowed_paths"])

    def test_match_command_rejects_blocked_prefix(self) -> None:
        policy = hook_common.normalize_policy({})
        allowed, reason = hook_common.match_command(policy, "rm -rf target")
        self.assertFalse(allowed)
        self.assertIn("blocked by prefix 'rm'", reason)

    def test_match_command_rejects_unknown_prefix(self) -> None:
        policy = hook_common.normalize_policy({})
        allowed, reason = hook_common.match_command(policy, "python3 setup.py")
        self.assertFalse(allowed)
        self.assertIn("is not in allowed_commands", reason)

    def test_match_path_allows_expected_and_blocks_other_paths(self) -> None:
        repo_root = self.create_repo()
        policy = hook_common.load_policy(repo_root)
        allowed, reason = hook_common.match_path(policy, ".harness/task.md", repo_root)
        self.assertTrue(allowed)
        self.assertIsNone(reason)
        allowed, reason = hook_common.match_path(policy, "src/main.py", repo_root)
        self.assertFalse(allowed)
        self.assertIn("outside allowed_paths", reason)

    def test_match_path_rejects_parent_escape(self) -> None:
        repo_root = self.create_repo()
        policy = hook_common.load_policy(repo_root)
        allowed, reason = hook_common.match_path(policy, "../AGENTS.md", repo_root)
        self.assertFalse(allowed)
        self.assertIn("outside allowed_paths", reason)

    def test_match_path_uses_segment_aware_glob_semantics(self) -> None:
        repo_root = self.create_repo()
        policy = hook_common.normalize_policy({"allowed_paths": ["src/*"], "blocked_paths": []})
        allowed, reason = hook_common.match_path(policy, "src/file.txt", repo_root)
        self.assertTrue(allowed)
        self.assertIsNone(reason)
        allowed, reason = hook_common.match_path(policy, "src/nested/file.txt", repo_root)
        self.assertFalse(allowed)
        self.assertIn("outside allowed_paths", reason)

    def test_collect_git_diff_and_budget_check_enforce_file_count(self) -> None:
        repo_root = self.create_repo()
        (repo_root / ".harness" / "policy.toml").write_text(
            "\n".join(
                [
                    'task_file = ".harness/task.md"',
                    'allowed_paths = [".harness/**"]',
                    "max_files_changed = 1",
                    "max_lines_added = 50",
                    "max_lines_deleted = 50",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        (repo_root / ".harness" / "one.txt").write_text("one\n", encoding="utf-8")
        (repo_root / ".harness" / "two.txt").write_text("two\n", encoding="utf-8")
        diff = hook_common.collect_git_diff(repo_root)
        reasons = hook_common.check_budget(hook_common.load_policy(repo_root), diff)
        self.assertTrue(any("max_files_changed" in reason for reason in reasons))

    def test_collect_git_diff_and_budget_check_enforce_added_and_deleted_lines(self) -> None:
        repo_root = self.create_repo()
        (repo_root / ".harness" / "baseline.txt").write_text("a\nb\nc\n", encoding="utf-8")
        self.git_commit(repo_root)
        (repo_root / ".harness" / "baseline.txt").write_text("a\n", encoding="utf-8")
        (repo_root / ".harness" / "policy.toml").write_text(
            "\n".join(
                [
                    'task_file = ".harness/task.md"',
                    'allowed_paths = [".harness/**"]',
                    "max_files_changed = 5",
                    "max_lines_added = 0",
                    "max_lines_deleted = 1",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        diff = hook_common.collect_git_diff(repo_root)
        reasons = hook_common.check_budget(hook_common.load_policy(repo_root), diff)
        self.assertTrue(any("max_lines_deleted" in reason for reason in reasons))

    def test_codex_payload_is_parsed_and_denied_with_official_shape(self) -> None:
        payload = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf target"},
            "cwd": str(REPO_ROOT),
            "transcript_path": str(REPO_ROOT / ".codex" / "sessions.jsonl"),
        }
        outcome = hook_common.evaluate_payload(payload, REPO_ROOT, expected_event="pre_tool")
        self.assertEqual(outcome["platform"], "codex")
        self.assertEqual(outcome["event"], "pre_tool")
        self.assertFalse(outcome["allowed"])
        self.assertEqual(
            outcome["response"]["hookSpecificOutput"]["permissionDecision"],
            "deny",
        )

    def test_claude_payload_is_parsed_and_denied_with_expected_shape(self) -> None:
        payload = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "python3 app.py"},
            "permission_mode": "default",
            "cwd": str(REPO_ROOT),
        }
        outcome = hook_common.evaluate_payload(payload, REPO_ROOT, expected_event="pre_tool")
        self.assertEqual(outcome["platform"], "claude")
        self.assertFalse(outcome["allowed"])
        self.assertEqual(
            outcome["response"]["hookSpecificOutput"]["permissionDecision"],
            "deny",
        )

    def test_gemini_payload_is_parsed_and_denied_with_expected_shape(self) -> None:
        payload = {
            "hook_event_name": "BeforeTool",
            "tool_name": "run_shell_command",
            "tool_input": {"command": "python3 app.py"},
            "cwd": str(REPO_ROOT),
        }
        outcome = hook_common.evaluate_payload(payload, REPO_ROOT, expected_event="pre_tool")
        self.assertEqual(outcome["platform"], "gemini")
        self.assertFalse(outcome["allowed"])
        self.assertEqual(outcome["response"]["decision"], "deny")

    def test_post_tool_path_violation_is_rejected(self) -> None:
        repo_root = self.create_repo()
        (repo_root / "src").mkdir()
        (repo_root / "src" / "main.py").write_text("print('hi')\n", encoding="utf-8")
        payload = {
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": "src/main.py"},
            "permission_mode": "default",
            "cwd": str(repo_root),
        }
        outcome = hook_common.evaluate_payload(payload, repo_root, expected_event="post_tool")
        self.assertFalse(outcome["allowed"])
        self.assertIn("outside allowed_paths", outcome["reason"])

    def test_entrypoints_reject_mismatched_events(self) -> None:
        payload = {
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": "src/main.py"},
            "permission_mode": "default",
            "cwd": str(REPO_ROOT),
        }
        with self.assertRaisesRegex(ValueError, "does not match entrypoint"):
            hook_common.evaluate_payload(payload, REPO_ROOT, expected_event="pre_tool")

    def test_main_writes_structured_log_entry(self) -> None:
        repo_root = self.create_repo()
        payload = {
            "hook_event_name": "BeforeTool",
            "tool_name": "run_shell_command",
            "tool_input": {"command": "python3 app.py"},
            "cwd": str(repo_root),
        }
        env = os.environ.copy()
        env["HARNESS_PLATFORM"] = "gemini"
        result = subprocess.run(
            [sys.executable, ".harness/hooks/pre_tool.py"],
            cwd=repo_root,
            input=json.dumps(payload),
            text=True,
            capture_output=True,
            env=env,
            check=True,
        )
        self.assertEqual(json.loads(result.stdout)["decision"], "deny")
        log_files = list((repo_root / ".harness" / "logs").glob("*.jsonl"))
        self.assertEqual(len(log_files), 1)
        entry = json.loads(log_files[0].read_text(encoding="utf-8").splitlines()[0])
        self.assertEqual(entry["platform"], "gemini")
        self.assertEqual(entry["event"], "pre_tool")
        self.assertFalse(entry["allowed"])


if __name__ == "__main__":
    unittest.main()
