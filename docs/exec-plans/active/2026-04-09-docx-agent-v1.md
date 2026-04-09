# ExecPlan: docx-agent-v1

## Objective

Implement the first real product feature: a DOCX expansion CLI using `rig`, OpenRouter, optional user-provided URLs, and internet search.

## Context

- The repository is currently a single-crate Rust project.
- The user wants the repository root to become a workspace for future multi-agent expansion.
- The first feature must use `rig`, OpenRouter, and one search backend.
- User input is a document plus a prompt; the agent should decide whether search is needed.

## Decisions

- Convert the root into a Cargo workspace.
- Create three members: `agent-core`, `docx-agent`, and `docx-cli`.
- Support `.docx` only in v1.
- Use OpenRouter as the only real LLM provider in v1.
- Use Tavily as the only real search backend in v1.
- Keep search and URL fetching behind traits for future expansion.
- v1 uses a simple search-decision policy: user-provided URLs are always fetched, and Tavily search is triggered only when the prompt implies external research is needed.
- Runtime configuration is stored in a repository-root `agent.toml` file, while `agent.example.toml` is the committed template and `.gitignore` prevents real secrets from being committed.
- Search uses a Tavily API key; when it is not configured, the agent degrades gracefully to document plus user-provided URLs only.

## Steps

1. Convert the repository to a workspace layout.
2. Add shared core types and traits.
3. Implement DOCX parsing, URL fetch, Tavily search, and OpenRouter agent execution.
4. Add the CLI entrypoint and repository-root TOML configuration.
5. Add runtime logging and validate real end-to-end execution from the repository root.

## Validation

- `cargo fmt`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo run -p docx-cli -- expand --doc ./e2e-docx-agent-test.docx --prompt '请基于文档内容扩写为一版更完整的中文产品说明，不要联网搜索。' --output ./e2e-docx-agent-output-nosearch.md`
- `cargo run -p docx-cli -- expand --doc ./e2e-docx-agent-test.docx --prompt '请联网搜索 AI 文档助手产品能力，并基于文档内容扩写为一版更完整的中文产品说明。' --output ./e2e-docx-agent-output.md`

## Review Gate

- Review Status: pending_human_review
- Files Intended For Commit: `.gitignore`, `Cargo.toml`, `Cargo.lock`, `agent.example.toml`, `apps/`, `crates/`, `src/main.rs` removal, `docs/exec-plans/active/2026-04-09-docx-agent-v1.md`
- Proposed Commit Message: `Add DOCX expansion agent workspace`
- Human Approval: pending explicit approval in chat

## Progress Log

- 2026-04-09: created the implementation plan for the first DOCX expansion feature.
- 2026-04-09: converted the repository into a Cargo workspace with `agent-core`, `docx-agent`, and `docx-cli`.
- 2026-04-09: implemented DOCX heading/paragraph parsing, search integration, URL content fetching, and an OpenRouter-backed `rig` generation flow.
- 2026-04-09: added CLI input/output handling for `docx-cli expand --doc --prompt [--url] [--output]`.
- 2026-04-09: replaced environment-based runtime settings with repository-root `agent.toml` loading and added `--config` override support in the CLI.
- 2026-04-09: hardened the review findings by ignoring local `agent.toml`, committing `agent.example.toml`, validating placeholder secrets, resolving the default config path from the repository root, and degrading gracefully when search is unavailable.
- 2026-04-09: replaced the earlier search backend choice with a Tavily API-based integration better suited to reliable agent search.
- 2026-04-09: validated the Tavily-backed implementation with `cargo fmt --all`, `cargo check --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets --all-features`.
- 2026-04-09: validation passed with `cargo fmt --all`, `cargo check --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets --all-features`.
- 2026-04-09: added `tracing`-based runtime logs for config loading, DOCX parsing, Tavily search, and OpenRouter generation to avoid black-box failures during CLI runs.
- 2026-04-09: compressed the generation system prompt, fixed the heuristic so prompts like `不要联网搜索` no longer trigger Tavily, and added retry/backoff for transient OpenRouter rate limits.
- 2026-04-09: generated `/Volumes/mian/code/rs/agent-rs/e2e-docx-agent-test.docx` in the repository root and verified successful no-search and Tavily-enabled end-to-end runs that wrote `/Volumes/mian/code/rs/agent-rs/e2e-docx-agent-output-nosearch.md` and `/Volumes/mian/code/rs/agent-rs/e2e-docx-agent-output.md`.
- 2026-04-09: fixed pre-commit review findings by preserving DOCX run word continuity, improving HTML body extraction to keep block word boundaries without splitting inline words, and enforcing `source_chars` truncation on Tavily search content.

## Risks / Follow-Ups

- DOCX parsing in v1 will favor robust plain-text extraction over Word fidelity.
- Search activation in v1 is heuristic and prompt-driven; later iterations can replace it with a model-directed tool loop if needed.
- Web page extraction in v1 is text-first and does not run a full readability pipeline.
