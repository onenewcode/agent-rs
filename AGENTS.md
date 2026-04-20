# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace for DOCX expansion with advanced observability and tool-calling capabilities. 
- `apps/docx-cli/`: CLI entrypoint.
- `crates/agent-kernel/`: Core abstractions, type-safe context (`TypeMap`), and state-machine execution contracts.
- `crates/agent-runtime/`: Workflow orchestration with retries and fallbacks.
- `crates/agent-tools/`: Reusable, domain-agnostic AI tools (Search, Edit).
- `crates/docx-domain/`: DOCX-specific logic and multi-dimensional LLM evaluation.
- `crates/agent-adapters/`: Infrastructure (OpenRouter, Tavily) with telemetry instrumentation.

## Build, Test, and Development Commands
Use workspace commands from the repo root:

- `cargo build --workspace`: Build all components.
- `cargo test --workspace`: Run unit and resilience tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: Enforce lint standards.
- `cargo run -p docx-cli -- run --doc <PATH> --prompt "<PROMPT>"`: Execute local expansion.

## Coding Style & Naming Conventions
- **Edition**: Rust 2024.
- **Context Management**: Always use the typed `WorkflowContext` API. Never pass raw JSON between steps for state.
- **Error Handling**: `thiserror` for library crates; `anyhow` for the CLI.
- **Async**: Use `tokio` and `JoinSet` for maximizing I/O parallelism (especially in `ResearchStep`).

## Testing Guidelines
- **Unit Tests**: Inline alongside implementation.
- **Resilience Testing**: Always test workflows against mock failures to ensure `RetryPolicy` and `fallback_step` work as expected.
- **Observability Verification**: Verify that `Telemetry` and `AgentTrajectory` are correctly populated in tests.

## Commit & Pull Request Guidelines
- **Conventional Commits**: Use `feat:`, `fix:`, `refactor:`, `docs:`, etc.
- **PR Description**: Detail architectural changes, token/cost impact, and provide evidence of passing tests.

## Configuration & Secrets
- `agent.example.toml`: Reference for setting up local `agent.toml`.
- **Telemetry**: Configure model pricing in TOML for accurate cost estimation.
- **Security**: Never commit `agent.toml` or any files containing API keys.
