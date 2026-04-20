# Observability & Monitoring

`agent-rs` implements comprehensive telemetry following modern LLMOps standards.

## Telemetry

The `Telemetry` state tracks resource consumption throughout the workflow:
- **Token Usage**: Split into `prompt_tokens` and `completion_tokens` using `tiktoken-rs`.
- **Cost Estimation**: Calculated based on per-1M-token pricing configured in `agent.toml`.
- **Latency**: Recorded for each step in the `RunReport`.

## Agent Trajectory

The `AgentTrajectory` captures the behavioral history of the agent during tool-calling loops.
- **Thought**: The reasoning process before an action.
- **Action**: Which tool was called, what were the input arguments, and what was the outcome.

This is critical for debugging why an agent made a specific edit or why it got stuck in a research loop.

## Structured Logging

We use the `tracing` crate. Set `RUST_LOG=info` to see real-time step transitions, tool calls, and model costs in the terminal.
