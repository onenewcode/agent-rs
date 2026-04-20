# Getting Started for Developers

## Prerequisites

- Rust 1.85+ (2024 Edition)
- `uv` (for maintenance scripts)

## Development Workflow

1.  **Environment Setup**:
    ```bash
    cp agent.example.toml agent.toml
    # Edit agent.toml with your OpenRouter/Tavily keys
    ```

2.  **Common Commands**:
    - `cargo check --workspace`: Rapid compilation check.
    - `cargo test --workspace`: Run the full test suite (highly recommended).
    - `cargo clippy --workspace -- -D warnings`: Check for common Rust pitfalls.
    - `RUST_LOG=info cargo run -p docx-cli -- run --doc tests/input.docx --prompt "..."`: Run a local task with full observability logs.

3.  **Adding a New Tool**:
    - Define a struct in `crates/agent-tools/src/lib.rs`.
    - Implement `rig::tool::Tool`.
    - Update `docx-domain/src/workflow.rs` to register the tool in the relevant step.

4.  **Adding a New Step**:
    - Create a struct that implements `WorkflowStep` in the domain crate.
    - Update the `WorkflowDefinition` in the `Workflow::build` method.
    - (Optional) Configure retries and fallback in the definition.
