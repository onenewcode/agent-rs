# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace for DOCX expansion and evaluation. Use `apps/docx-cli/` for the CLI entrypoint, `crates/agent-kernel/` for stable runtime contracts and core types, `crates/agent-runtime/` for workflow orchestration, `crates/docx-domain/` for DOCX parsing and prompt construction, and `crates/agent-adapters/` for OpenRouter, Tavily, fetching, and caching adapters. Keep planning notes in `docs/exec-plans/`. Treat `target/` as generated output.

## Build, Test, and Development Commands
Use workspace commands from the repo root:

- `cargo build --workspace` builds all crates.
- `cargo test --workspace` runs the current unit and async test suites.
- `cargo run -p docx-cli -- expand --doc test_input.docx --prompt "Expand this section"` runs the CLI locally.
- `cargo fmt --all` applies standard Rust formatting.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` enforces the repo lint bar.
- `uv run .harness/sync.py` refreshes harness-managed docs or generated metadata when relevant.

## Coding Style & Naming Conventions
The workspace uses Rust 2024 edition with `clippy::all` and `clippy::pedantic` enabled at the workspace level. Follow `rustfmt` defaults: 4-space indentation, trailing commas where formatted, and small focused modules. Use `snake_case` for files, modules, functions, and config keys; `CamelCase` for structs, enums, and traits; `SCREAMING_SNAKE_CASE` for constants. Prefer `anyhow` in app code, `thiserror` in library crates, and `tracing` for operational logs.

## Testing Guidelines
Tests currently live inline beside implementation under `#[cfg(test)]`, with async cases using `#[tokio::test]`. Add tests in the same module when changing parsing, fetch, cache, orchestration, or prompt-building logic. Run `cargo test --workspace` before opening a PR, and use targeted runs such as `cargo test -p agent-runtime` while iterating.

## Commit & Pull Request Guidelines
Recent history uses conventional prefixes such as `feat:` and `fix:`. Keep commit subjects imperative and scoped to one logical change. Pull requests should explain the behavior change, list affected crates, note any config updates, and include the exact validation commands you ran. For CLI-output changes, include a short sample invocation or output snippet.

## Configuration & Secrets
Start from `agent.example.toml` and keep local secrets in `agent.toml`. Never commit API keys or provider credentials. Default cache output goes to `.agent-cache/`; avoid checking in machine-specific artifacts.
