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
            ".harness/templates",
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
            HARNESS_DIR / "templates" / "instruction.md.tmpl": repo_root / ".harness" / "templates" / "instruction.md.tmpl",
            HARNESS_DIR / "templates" / "commands.md.tmpl": repo_root / ".harness" / "templates" / "commands.md.tmpl",
        }
        for source, destination in copy_map.items():
            shutil.copy2(source, destination)

        return repo_root

    def run_event(
        self,
        repo_root: Path,
        event: str,
        command: str,
        platform: str = "codex",
    ) -> subprocess.CompletedProcess[str]:
        payload = json.dumps({"tool_input": {"command": command}})
        return subprocess.run(
            [sys.executable, ".harness/hooks/policy_hook.py", event, platform],
            cwd=repo_root,
            input=payload,
            text=True,
            capture_output=True,
            check=False,
        )

    def init_git_repo(self, repo_root: Path) -> None:
        subprocess.run(["git", "init"], cwd=repo_root, check=True, capture_output=True, text=True)
        subprocess.run(["git", "config", "user.name", "Harness Test"], cwd=repo_root, check=True, capture_output=True, text=True)
        subprocess.run(
            ["git", "config", "user.email", "harness-test@example.com"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
        readme = repo_root / "README.md"
        readme.write_text("seed\n", encoding="utf-8")
        subprocess.run(["git", "add", "README.md"], cwd=repo_root, check=True, capture_output=True, text=True)
        subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, check=True, capture_output=True, text=True)

    def set_diff_budget(self, repo_root: Path, enabled: bool, files: int, added: int, deleted: int) -> None:
        path = repo_root / ".harness" / "policy.toml"
        old = (
            "[diff_budget]\n"
            "enabled = false\n"
            "max_files_changed = 9999\n"
            "max_lines_added = 200000\n"
            "max_lines_deleted = 200000"
        )
        new = (
            "[diff_budget]\n"
            f"enabled = {str(enabled).lower()}\n"
            f"max_files_changed = {files}\n"
            f"max_lines_added = {added}\n"
            f"max_lines_deleted = {deleted}"
        )
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text)
        path.write_text(text.replace(old, new), encoding="utf-8")

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
            ".harness/instructions.md",
            ".codex/hooks.json",
            ".codex/rules/commands.md",
            ".claude/settings.json",
            ".gemini/settings.json",
        ):
            self.assertTrue((repo_root / relative).exists(), relative)
        for relative in ("AGENTS.md", "CLAUDE.md", "GEMINI.md"):
            self.assertFalse((repo_root / relative).exists(), relative)

    def test_sync_is_idempotent(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        snapshot = {
            relative: (repo_root / relative).read_text(encoding="utf-8")
            for relative in (
                ".harness/instructions.md",
                ".codex/hooks.json",
                ".codex/rules/commands.md",
                ".claude/settings.json",
                ".gemini/settings.json",
            )
        }
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        current = {relative: (repo_root / relative).read_text(encoding="utf-8") for relative in snapshot}
        self.assertEqual(snapshot, current)

    def test_agent_files_contain_single_shared_reference(self) -> None:
        repo_root = self.create_repo()
        for relative in ("AGENTS.md", "CLAUDE.md", "GEMINI.md"):
            (repo_root / relative).write_text("placeholder\n", encoding="utf-8")
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        for relative in ("AGENTS.md", "CLAUDE.md", "GEMINI.md"):
            content = (repo_root / relative).read_text(encoding="utf-8")
            self.assertEqual(content.count(".harness/instructions.md"), 1, relative)
            self.assertNotIn("## Policy Summary", content)

    def test_shared_instructions_contain_policy_summary(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        content = (repo_root / ".harness" / "instructions.md").read_text(encoding="utf-8")
        self.assertIn("## Policy Summary", content)
        self.assertIn("Current goals:", content)

    def test_codex_hooks_use_single_runner(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        hooks = json.loads((repo_root / ".codex" / "hooks.json").read_text(encoding="utf-8"))
        self.assertIn("Stop", hooks["hooks"])
        pre = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        stop = hooks["hooks"]["Stop"][0]["hooks"][0]["command"]
        self.assertIn("policy_hook.py PreToolUse codex", pre)
        self.assertIn("policy_hook.py Stop codex", stop)

    def test_commands_doc_matches_unknown_policy(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        content = (repo_root / ".codex" / "rules" / "commands.md").read_text(encoding="utf-8")
        self.assertIn("unknown_command = allow_warn", content)
        self.assertNotIn("Commands outside the allow list are rejected.", content)

    def test_policy_hook_allows_read_command(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "git status")
        self.assertEqual(result.returncode, 0)

    def test_policy_hook_blocks_destructive_command_in_compound(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "git status && rm -rf target")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Blocked destructive command prefix", result.stdout)

    def test_policy_hook_blocks_git_path_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PostToolUse", "git add .git/config")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Write to blocked path detected", result.stdout)

    def test_policy_hook_allows_unknown_with_warning(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "python3 app.py")
        self.assertEqual(result.returncode, 0)
        self.assertIn("Unknown command classification allowed with warning", result.stderr)

    def test_diff_budget_denies_when_enabled_and_exceeded(self) -> None:
        repo_root = self.create_repo()
        self.init_git_repo(repo_root)
        self.set_diff_budget(repo_root, enabled=True, files=0, added=0, deleted=0)

        readme = repo_root / "README.md"
        readme.write_text("seed\nchange\n", encoding="utf-8")

        result = self.run_event(repo_root, "PostToolUse", "mkdir -p .harness/tmp")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Diff budget exceeded", result.stdout)

    def test_policy_hook_writes_structured_log(self) -> None:
        repo_root = self.create_repo()
        self.run_event(repo_root, "PreToolUse", "git status", platform="codex")
        logs = sorted((repo_root / ".harness" / "logs").glob("*.jsonl"))
        self.assertTrue(logs)
        entry = json.loads(logs[-1].read_text(encoding="utf-8").splitlines()[-1])
        self.assertEqual(entry["event"], "PreToolUse")
        self.assertEqual(entry["platform"], "codex")
        self.assertIn("policy_version", entry)
        self.assertIn("decision_latency_ms", entry)
        self.assertIn("metadata", entry)


if __name__ == "__main__":
    unittest.main()
