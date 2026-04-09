# Local Runtime Recommendations

This repository does not commit a `.codex/config.toml`.

## Required Local Steps

1. Enable the `codex_hooks` feature so `.codex/hooks.json` is honored.
2. Point git at the repo hooks path:
   `git config core.hooksPath .githooks`
3. Ensure a Python 3 interpreter, `cargo fmt`, and `cargo clippy` are available locally.
4. Restart Codex after changing local hook-related settings.
