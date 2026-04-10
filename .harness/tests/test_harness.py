from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_DIR = REPO_ROOT / ".harness"


class HarnessTestCase(unittest.TestCase):
    def create_repo(self) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        repo_root = Path(temp_dir.name)

        for relative in (
            ".harness/logs",
            ".codex",
            ".claude",
            ".gemini",
        ):
            (repo_root / relative).mkdir(parents=True, exist_ok=True)

        shutil.copytree(
            HARNESS_DIR / "hooks",
            repo_root / ".harness" / "hooks",
            dirs_exist_ok=True,
            ignore=shutil.ignore_patterns("__pycache__"),
        )

        copy_map = {
            HARNESS_DIR / "policy.toml": repo_root / ".harness" / "policy.toml",
            HARNESS_DIR / "task.md": repo_root / ".harness" / "task.md",
            HARNESS_DIR / "sync.py": repo_root / ".harness" / "sync.py",
        }
        for source, destination in copy_map.items():
            shutil.copy2(source, destination)

        return repo_root

    def run_hook(self, repo_root: Path, entry: str, command: str) -> subprocess.CompletedProcess[str]:
        payload = json.dumps({"tool_input": {"command": command}})
        return subprocess.run(
            [sys.executable, f".harness/hooks/{entry}"],
            cwd=repo_root,
            input=payload,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_sync_generates_all_outputs(self) -> None:
        repo_root = self.create_repo()
        result = subprocess.run(
            [sys.executable, ".harness/sync.py"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0)
        for relative in (
            "AGENTS.md",
            "CLAUDE.md",
            "GEMINI.md",
            ".codex/hooks.json",
            ".claude/settings.json",
            ".gemini/settings.json",
        ):
            self.assertTrue((repo_root / relative).exists(), relative)

    def test_sync_is_idempotent(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        snapshot = {
            relative: (repo_root / relative).read_text(encoding="utf-8")
            for relative in (
                "AGENTS.md",
                "CLAUDE.md",
                "GEMINI.md",
                ".codex/hooks.json",
                ".claude/settings.json",
                ".gemini/settings.json",
            )
        }
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        current = {
            relative: (repo_root / relative).read_text(encoding="utf-8")
            for relative in snapshot
        }
        self.assertEqual(snapshot, current)

    def test_codex_hooks_include_stop(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        hooks = json.loads((repo_root / ".codex" / "hooks.json").read_text(encoding="utf-8"))
        self.assertIn("Stop", hooks["hooks"])
        self.assertIn("stop.py", hooks["hooks"]["Stop"][0]["hooks"][0]["command"])

    def test_policy_hook_allows_read_command(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "pre_tool.py", "git status")
        self.assertEqual(result.returncode, 0)

    def test_policy_hook_blocks_destructive_command(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "pre_tool.py", "rm -rf target")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Blocked destructive command prefix", result.stdout)

    def test_policy_hook_blocks_git_path_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "post_tool.py", "git add .git/config")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Write to blocked path detected", result.stdout)

    def test_policy_hook_allows_unknown_with_warning(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "pre_tool.py", "python3 app.py")
        self.assertEqual(result.returncode, 0)
        self.assertIn("Unknown command classification allowed with warning", result.stderr)

    def test_post_hook_allows_normal_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "post_tool.py", "mkdir -p .harness/tmp")
        self.assertEqual(result.returncode, 0)

    def test_stop_hook_blocks_git_path_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_hook(repo_root, "stop.py", "git add .git/config")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Write to blocked path detected", result.stdout)

    def test_policy_hook_writes_structured_log(self) -> None:
        repo_root = self.create_repo()
        self.run_hook(repo_root, "pre_tool.py", "git status")
        logs = sorted((repo_root / ".harness" / "logs").glob("*.jsonl"))
        self.assertTrue(logs)
        entry = json.loads(logs[-1].read_text(encoding="utf-8").splitlines()[-1])
        self.assertIn("event", entry)
        self.assertIn("decision", entry)
        self.assertIn("reason", entry)
        self.assertEqual(entry["event"], "PreToolUse")


if __name__ == "__main__":
    unittest.main()
