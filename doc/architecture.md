# System Architecture

`agent-rs` is designed as a decoupled workspace consisting of several specialized crates.

## Crate Structure

| Crate | Responsibility |
| :--- | :--- |
| `agent-kernel` | **Core Contracts**. Defines `WorkflowStep`, `LanguageModel`, `TypeMap`, and `Telemetry` types. No provider implementation. |
| `agent-runtime` | **Orchestration**. Implements the state-machine execution loop, retry logic, and fallback handling. |
| `agent-tools` | **AI Tools**. Domain-agnostic implementations of `rig::tool::Tool` (e.g., `EditDocumentTool`, `WebSearchTool`). |
| `docx-domain` | **Business Logic**. Specific workflow for DOCX expansion, document parsing, and evaluation rubrics. |
| `agent-adapters` | **Infrastructure**. Adapters for OpenRouter, Tavily, and HTTP fetching. Handles telemetry instrumentation. |
| `agent-app` | **DI / Wiring**. Loads TOML config and initializes the `PlatformApp` container. |
| `apps/docx-cli` | **Entrypoint**. Thin CLI wrapper. |

## Typed Context Management

We use a `TypeMap` within `WorkflowContext` to pass state between steps. This avoids the overhead of JSON serialization and provides compile-time safety.

```rust
// In a Step
let doc = context.state::<Document>()?; // Read
context.insert_state(updated_draft);     // Write
```

## State-Machine Runtime

Workflows are not linear lists. They are directed graphs defined by `StepTransition`.

- `StepTransition::Next("step_id")`: Continues to the next node.
- `StepTransition::Complete { ... }`: Terminates the workflow with a report.

This allows for complex loops (e.g., Generate -> Evaluate -> Refine -> Evaluate).
