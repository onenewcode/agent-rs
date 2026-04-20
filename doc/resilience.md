# Exception Handling & Recovery

`agent-rs` is built to handle the inherent instability of web scraping and LLM APIs.

## Retry Policies

Every step in the workflow can be configured with a `RetryPolicy`.

```rust
StepConfig::new(Arc::new(ResearchStep))
    .with_retry(3, 1000) // Max 3 attempts, 1s base delay
```

The runtime performs exponential backoff automatically: `1s -> 2s -> 4s`.

## Fallback Steps

If retries are exhausted, the runtime can transition to a `fallback_step` instead of failing the entire run.

```rust
StepConfig::new(Arc::new(DeepSearch))
    .with_fallback("SimpleSearch")
```

## Graceful Degradation

If an optional step (like search results) fails, the system logs a warning but continues with a partial set of data, ensuring the agent still delivers a result even if the environment is unstable.
