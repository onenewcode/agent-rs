# Gemini Instructions - agent-rs

## Project Overview
`agent-rs` is a Rust-based toolkit designed to expand DOCX documents using Large Language Models (LLMs) and supporting research from the web. It provides a modular architecture for parsing documents, fetching web content, performing searches, and orchestrating the expansion process using an iterative, tool-calling refinement loop.

### Main Technologies
- **Rust (2024 edition)**: Core language.
- **Tokio**: Asynchronous runtime.
- **Reqwest**: HTTP client for web fetching and API calls.
- **Rig (rig-core)**: LLM abstraction framework for agentic workflows and tool calling.
- **OpenRouter**: Preferred LLM provider gateway.
- **Tavily**: Optional search backend for external research.
- **roxmltree & zip**: Used for parsing `.docx` (OpenXML) files.
- **Serde**: For serialization and configuration.
- **Tiktoken-rs**: For precise token estimation and cost monitoring.

### Architecture
The workspace is divided into seven main components:
- **`crates/agent-kernel`**: Defines generic workflow contracts, state-machine execution primitives, telemetry types, and core service traits. Uses a type-safe `TypeMap` for context management.
- **`crates/agent-runtime`**: Executes registered workflows as a directed graph of steps with support for declarative retries, exponential backoff, and fallback transitions.
- **`crates/agent-tools`**: Reusable AI tools (e.g., `EditDocumentTool`, `WebSearchTool`) that implement the `rig::tool::Tool` trait.
- **`crates/docx-domain`**: Owns DOCX parsing, multi-dimensional evaluation (Faithfulness, Relevance, Accuracy), and the tool-enabled `docx.expand` refinement loop.
- **`crates/agent-adapters`**: Implements infrastructure adapters for OpenRouter, Tavily, web fetching, and JSON report persistence with integrated telemetry.
- **`crates/agent-app`**: Loads canonical TOML config and wires the application container.
- **`apps/docx-cli`**: Command-line entrypoint for dispatching expansion requests.

---

## Building and Running

### Prerequisites
- Rust (latest stable, 2024 edition support)
- `uv` (for running maintenance scripts)

### Key Commands
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`
- **Run (CLI)**:
  ```bash
  cargo run -p docx-cli -- run --doc <PATH_TO_DOCX> --prompt "<EXPANSION_PROMPT>" --url <URL1>
  ```
- **Linting**:
  ```bash
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo fmt --all -- --check
  ```

---

## Development Conventions

### Coding Style
- **Type-Safe Context**: Use `context.insert_state(T)` and `context.state::<T>()` for cross-step communication. Avoid raw JSON for internal logic.
- **Agentic Refinement**: Prefer localized tool-based edits over full text regeneration in refinement steps.
- **Resilience**: Configure `RetryPolicy` and `fallback_step` for I/O heavy operations.
- **Observability**: Ensure every model call and tool action is recorded in `Telemetry` or `AgentTrajectory`.

### Testing Practices
- **Unit Tests**: Inline under `#[cfg(test)]`.
- **Resilience Tests**: Mock failing providers in `agent-runtime` to verify retry and fallback behavior.

### Crate Boundaries
- `agent-kernel`: Dependency-light, no provider-specific code.
- `agent-adapters`: Handles external APIs and telemetry instrumentation.
- `agent-tools`: Domain-agnostic tool implementations.
- `docx-domain`: Specific business logic for document processing.
