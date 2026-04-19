# Gemini Instructions - agent-rs

## Project Overview
`agent-rs` is a Rust-based toolkit designed to expand DOCX documents using Large Language Models (LLMs) and supporting research from the web. It provides a modular architecture for parsing documents, fetching web content, performing searches, and orchestrating the expansion process.

### Main Technologies
- **Rust (2024 edition)**: Core language.
- **Tokio**: Asynchronous runtime.
- **Reqwest**: HTTP client for web fetching and API calls.
- **Rig (rig-core)**: LLM abstraction framework.
- **OpenRouter**: Preferred LLM provider gateway.
- **Tavily**: Optional search backend for external research.
- **roxmltree & zip**: Used for parsing `.docx` (OpenXML) files.
- **Serde**: For serialization and configuration.

### Architecture
The workspace is divided into five main components:
- **`crates/agent-kernel`**: Defines stable runtime contracts (`Planner`, `Researcher`, `Generator`, `Evaluator`, `Refiner`, `SearchProvider`, `SourceFetcher`) and shared data models (`Task`, `Plan`, `Draft`, `Evaluation`, `RunReport`).
- **`crates/agent-runtime`**: Runs the explicit workflow (`plan -> research -> generate -> evaluate -> refine`) and records stage-level telemetry.
- **`crates/docx-domain`**: Owns DOCX parsing, token budgeting, and prompt rendering for DOCX-centered tasks.
- **`crates/agent-adapters`**: Implements OpenRouter, Tavily, webpage fetching, caching, and DOCX-specific stage components.
- **`apps/docx-cli`**: A thin command-line composition root for loading config, wiring adapters, and executing the runtime.

---

## Building and Running

### Prerequisites
- Rust (latest stable, 2024 edition support)
- `uv` (for running Python-based harness scripts)

### Key Commands
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`
- **Run (CLI)**:
  ```bash
  cargo run -p docx-cli -- expand --doc <PATH_TO_DOCX> --prompt "<EXPANSION_PROMPT>" --url <URL1> --url <URL2>
  ```
- **Linting**:
  ```bash
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo fmt --all -- --check
  ```
- **Maintenance (Harness Sync)**:
  ```bash
  uv run .harness/sync.py
  ```

### Configuration
The project uses a TOML configuration file. An example is provided in `agent.example.toml`. Copy it to `agent.toml` and add your API keys:
- `providers.generator.api_key`: OpenRouter API key for planning/generation/refinement.
- `providers.evaluator.api_key`: OpenRouter API key for evaluation.
- `providers.search.api_key`: Tavily API key (optional).

---

## Development Conventions

### Coding Style
- **Immutability**: Prefer creating new objects over mutating existing ones, especially in core logic.
- **Error Handling**: Use `anyhow` for applications (CLI) and `thiserror` for library crates.
- **Tracing**: Use the `tracing` crate for logging. CLI logs are filtered via `RUST_LOG` or `EnvFilter`.
- **Lints**: The project enforces strict lints via `clippy::pedantic` and forbids `unsafe_code` by default (warn).

### Testing Practices
- **Unit Tests**: Located within each module.
- **Runtime Tests**: Workflow loop coverage is in `agent-runtime`.
- **Documentation**: Keep `docs/` and generated documentation in sync using `.harness/sync.py`.

### Edit Guardrails
- **Generated Files**: Never hand-edit files in `target/generated-preview/` or other generated documentation. Rerun `uv run .harness/sync.py`.
- **Crate Boundaries**: Keep `agent-kernel` dependency-light and stable; keep provider and IO details in `agent-adapters`.
