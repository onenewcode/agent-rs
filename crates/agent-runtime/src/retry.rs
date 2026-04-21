use agent_kernel::{Error, Result};
use std::future::Future;
use std::time::Duration;
use tracing::warn;

pub struct RetryPolicy {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 2000,
        }
    }
}

pub async fn retry_with_backoff<T, F, Fut, C>(
    name: &str,
    policy: &RetryPolicy,
    mut f: F,
    is_retryable: C,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
    C: Fn(&Error) -> bool,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match f().await {
            Ok(val) => return Ok(val),
            Err(error) => {
                if attempts < policy.max_attempts && is_retryable(&error) {
                    let delay =
                        policy.base_delay_ms * (2u64.pow(u32::try_from(attempts - 1).unwrap_or(0)));
                    warn!(
                        operation = name,
                        attempt = attempts,
                        delay_ms = delay,
                        error = %error,
                        "Operation failed; retrying with backoff"
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                } else {
                    return Err(error);
                }
            }
        }
    }
}
