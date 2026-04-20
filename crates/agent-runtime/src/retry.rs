use agent_kernel::RunError;
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
            max_attempts: 1,
            base_delay_ms: 1000,
        }
    }
}

pub async fn retry_with_backoff<T, F, Fut>(
    name: &str,
    policy: &RetryPolicy,
    mut f: F,
) -> Result<T, RunError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RunError>>,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match f().await {
            Ok(val) => return Ok(val),
            Err(error) => {
                if attempts < policy.max_attempts {
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
