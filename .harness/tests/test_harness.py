from __future__ import annotations

import json
import os
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
            "stop_hook.py",
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

    def run_stop(self, repo_root: Path, event: str = "Stop", env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, ".harness/hooks/stop_hook.py", event, "codex"],
            cwd=repo_root,
            input=json.dumps({}),
            text=True,
            capture_output=True,
            check=False,
            env=env,
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

    def test_sync_generates_stop_hook_command(self) -> None:
        repo_root = self.create_repo()
        subprocess.run([sys.executable, ".harness/sync.py"], cwd=repo_root, check=True)
        payload = json.loads((repo_root / ".codex" / "hooks.json").read_text(encoding="utf-8"))
        command = payload["hooks"]["Stop"][0]["hooks"][0]["command"]
        self.assertEqual(command, "python3 .harness/hooks/stop_hook.py Stop codex")

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

    def test_policy_hook_rejects_stop_event(self) -> None:
        repo_root = self.create_repo()
        payload = json.dumps({"tool_input": {"command": "git status"}})
        result = subprocess.run(
            [sys.executable, ".harness/hooks/policy_hook.py", "Stop", "codex"],
            cwd=repo_root,
            input=payload,
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("Unsupported hook event: Stop", result.stderr)

    def test_policy_hook_blocks_destructive_command_in_compound(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "git status && rm -rf target")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY blocked_prefix:rm")
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_destructive_command_with_env_assignment(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "FOO=1 rm -rf target")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY blocked_prefix:rm")
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_destructive_command_in_subshell(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", "echo ok $(rm -rf target)")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY blocked_prefix:rm")
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_git_path_write(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PostToolUse", "git add .git/config")
        self.assertEqual(result.returncode, 2)
        self.assertTrue(result.stdout.strip().startswith("DENY blocked_path:"))
        self.assertEqual(result.stderr, "")

    def test_policy_hook_blocks_unknown_command_writing_blocked_path(self) -> None:
        repo_root = self.create_repo()
        result = self.run_event(repo_root, "PreToolUse", 'sed -i "" -e s/a/b/ .git/config')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout.strip(), "DENY blocked_path:path:.git/config")
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

    def test_stop_hook_no_rust_files_is_noop(self) -> None:
        repo_root = self.create_repo()
        result = self.run_stop(repo_root)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")

    def test_stop_hook_warns_when_rust_exists_without_manifest(self) -> None:
        repo_root = self.create_repo()
        src = repo_root / "src"
        src.mkdir(parents=True, exist_ok=True)
        (src / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        result = self.run_stop(repo_root)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr.strip(), "WARN stop_no_cargo_manifest")

    def test_stop_hook_runs_fmt_then_clippy_fix(self) -> None:
        repo_root = self.create_repo()
        (repo_root / "Cargo.toml").write_text(
            dedent(
                """\
                [package]
                name = "demo"
                version = "0.1.0"
                edition = "2021"
                """
            ),
            encoding="utf-8",
        )
        src = repo_root / "src"
        src.mkdir(parents=True, exist_ok=True)
        (src / "lib.rs").write_text("pub fn f()->i32{1}\n", encoding="utf-8")

        fake_bin = repo_root / "fake-bin"
        fake_bin.mkdir(parents=True, exist_ok=True)
        log_path = repo_root / "cargo.log"
        fake_cargo = fake_bin / "cargo"
        fake_cargo.write_text(
            dedent(
                """\
                #!/bin/sh
                echo "$@" >> "$CARGO_LOG"
                case "$1" in
                  fmt) exit "${FMT_EXIT:-0}" ;;
                  clippy) exit "${CLIPPY_EXIT:-0}" ;;
                esac
                exit 0
                """
            ),
            encoding="utf-8",
        )
        fake_cargo.chmod(0o755)

        env = os.environ.copy()
        env["PATH"] = f"{fake_bin}:{env.get('PATH', '')}"
        env["CARGO_LOG"] = str(log_path)
        result = self.run_stop(repo_root, env=env)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")

        calls = log_path.read_text(encoding="utf-8").splitlines()
        self.assertGreaterEqual(len(calls), 2)
        self.assertEqual(calls[0], "fmt --all")
        self.assertTrue(calls[1].startswith("clippy --fix "))

    def test_stop_hook_denies_if_fmt_fails(self) -> None:
        repo_root = self.create_repo()
        (repo_root / "Cargo.toml").write_text(
            dedent(
                """\
                [package]
                name = "demo"
                version = "0.1.0"
                edition = "2021"
                """
            ),
            encoding="utf-8",
        )
        src = repo_root / "src"
        src.mkdir(parents=True, exist_ok=True)
        (src / "lib.rs").write_text("pub fn f()->i32{1}\n", encoding="utf-8")

        fake_bin = repo_root / "fake-bin"
        fake_bin.mkdir(parents=True, exist_ok=True)
        log_path = repo_root / "cargo.log"
        fake_cargo = fake_bin / "cargo"
        fake_cargo.write_text(
            dedent(
                """\
                #!/bin/sh
                echo "$@" >> "$CARGO_LOG"
                case "$1" in
                  fmt) echo "fmt failed" 1>&2; exit "${FMT_EXIT:-1}" ;;
                  clippy) exit "${CLIPPY_EXIT:-0}" ;;
                esac
                exit 0
                """
            ),
            encoding="utf-8",
        )
        fake_cargo.chmod(0o755)

        env = os.environ.copy()
        env["PATH"] = f"{fake_bin}:{env.get('PATH', '')}"
        env["CARGO_LOG"] = str(log_path)
        env["FMT_EXIT"] = "1"
        result = self.run_stop(repo_root, env=env)
        self.assertEqual(result.returncode, 2)
        self.assertTrue(result.stdout.strip().startswith("DENY stop_fmt_failed:"))
        self.assertEqual(result.stderr, "")

        calls = log_path.read_text(encoding="utf-8").splitlines()
        self.assertEqual(calls, ["fmt --all"])


if __name__ == "__main__":
    unittest.main()
