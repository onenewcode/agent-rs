# Developer Documentation

Welcome to the `agent-rs` developer documentation. This project is a highly modular, resilient, and observable AI agent toolkit built in Rust.

## Table of Contents

1.  **[Architecture Overview](./architecture.md)**
    *   Crate structure and responsibilities.
    *   Typed Context Management (`TypeMap`).
    *   State-Machine based Runtime.

2.  **[Agentic Design Patterns](./agentic-patterns.md)**
    *   Tool Calling and Localized Refinement.
    *   Standardized multi-dimensional Evaluation (LLM-as-a-Judge).
    *   Concurrency and Parallelism in Research.

3.  **[Observability & Monitoring](./observability.md)**
    *   Telemetry (Token usage, Cost, Latency).
    *   Agent Trajectory tracking.
    *   Structured Logging and Run Reports.

4.  **[Exception Handling & Recovery](./resilience.md)**
    *   Declarative Retry Policies.
    *   Fallback Transitions.
    *   Graceful Degradation.

5.  **[Getting Started Guide](./getting-started.md)**
    *   Development environment setup.
    *   Common cargo commands.
    *   Adding new Workflows and Tools.
