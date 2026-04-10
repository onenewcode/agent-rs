from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from textwrap import dedent


REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_DIR = REPO_ROOT / ".harness"


BASE_POLICY = dedent(
    """\
    [policy]
    version = 1

    [enforcement]
    scope = "writes_and_high_risk_only"
    unknown_command = "allow_warn"

    [paths]
    mode = "blocklist_only"
    blocked_write_paths = [".git/**"]

    [commands]
    allowed_prefixes = ["git status", "git diff", "git ls-files", "ls", "cat", "rg"]
    write_prefixes = ["git add", "git commit", "git mv", "git rm", "git apply", "touch", "mkdir", "cp", "mv", "tee", "truncate", "chmod", "chown", "ln"]
    blocked_prefixes = ["rm", "git reset", "git checkout", "git clean"]
    high_risk_prefixes = ["rm", "git reset", "git checkout", "git clean"]
    """
)


class HarnessTestCase(unittest.TestCase):
    def create_repo(self) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        repo_root = Path(temp_dir.name)

        for relative in (
            ".harness/templates",
            ".codex",
            ".claude",
            ".gemini",
        ):
            (repo_root / relative).mkdir(parents=True, exist_ok=True)

        required_hooks = (
            "command_policy.py",
            "hook_types.py",
            "path_policy.py",
            "policy_hook.py",
            "policy_io.py",
        )
        hooks_dir = repo_root / ".harness" / "hooks"
        hooks_dir.mkdir(parents=True, exist_ok=True)
        for filename in required_hooks:
            shutil.copy2(HARNESS_DIR / "hooks" / filename, hooks_dir / filename)

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

    def write_policy(self, repo_root: Path, text: str) -> None:
        (repo_root / ".harness" / "policy.toml").write_text(text, encoding="utf-8")

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

    def test_commands_doc_includes_write_prefixes_and_output_contract(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        content = (repo_root / ".codex" / "rules" / "commands.md").read_text(encoding="utf-8")
        self.assertIn("## Write Command Prefixes", content)
        self.assertIn("unknown_command = allow_warn", content)
        self.assertIn("`WARN <code>`", content)
        self.assertIn("`DENY <code>[:detail]`", content)

    def test_policy_hook_allows_read_command_and_is_silent(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "git status")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_destructive_command_in_compound(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "git status && rm -rf target")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY blocked_prefix:rm")
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_git_path_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PostToolUse", "git add .git/config")
        self.assertEqual(result.returncode, 2)
        self.assertTrue(result.stdout.strip().startswith("DENY blocked_path:"))
        self.assertEqual(result.stderr, "")

    def test_policy_hook_allows_unknown_with_warning_and_single_line(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "python3 app.py")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr.strip(), "WARN unknown_command")
        self.assertEqual(len(result.stderr.strip().splitlines()), 1)

    def test_allowed_prefixes_are_policy_driven(self) -> None:
        repo_root = self.create_repo()
        self.write_policy(
            repo_root,
            BASE_POLICY.replace(
                'allowed_prefixes = ["git status", "git diff", "git ls-files", "ls", "cat", "rg"]',
                'allowed_prefixes = ["find"]',
            ).replace('unknown_command = "allow_warn"', 'unknown_command = "deny"'),
        )
        allowed = self.run_event(repo_root, "PreToolUse", "find . -maxdepth 1")
        denied = self.run_event(repo_root, "PreToolUse", "git status")
        self.assertEqual(allowed.returncode, 0)
        self.assertEqual(allowed.stdout, "")
        self.assertEqual(allowed.stderr, "")
        self.assertEqual(denied.returncode, 2)
        self.assertEqual(denied.stdout.strip(), "DENY unknown_command")

    def test_high_risk_prefixes_are_policy_driven(self) -> None:
        repo_root = self.create_repo()
        custom = (
            BASE_POLICY.replace(
                'high_risk_prefixes = ["rm", "git reset", "git checkout", "git clean"]',
                'high_risk_prefixes = ["sudo"]',
            )
            .replace('blocked_prefixes = ["rm", "git reset", "git checkout", "git clean"]', "blocked_prefixes = []")
            .replace('unknown_command = "allow_warn"', 'unknown_command = "deny"')
        )
        self.write_policy(repo_root, custom)
        result = self.run_event(repo_root, "PreToolUse", "sudo ls")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")

    def test_invalid_policy_denies_with_short_code(self) -> None:
        repo_root = self.create_repo()
        invalid = BASE_POLICY.replace(
            'write_prefixes = ["git add", "git commit", "git mv", "git rm", "git apply", "touch", "mkdir", "cp", "mv", "tee", "truncate", "chmod", "chown", "ln"]\n',
            "",
        )
        self.write_policy(repo_root, invalid)
        result = self.run_event(repo_root, "PreToolUse", "git status")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY invalid_policy:write_prefixes")
        self.assertEqual(result.stderr, "")


if __name__ == "__main__":
    unittest.main()
