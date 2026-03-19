//! Retry with exponential backoff + jitter for async operations.

use std::future::Future;
use std::time::Duration;
use rand::Rng;

/// Configuration for retry behaviour.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
            jitter: true,
        }
    }
}

/// Retry an async closure with exponential backoff.
///
/// The closure receives the attempt number (0-based) and must return
/// `Result<T, E>` where `E: std::fmt::Display`.
pub async fn retry_async<F, Fut, T, E>(
    config: &RetryConfig,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut delay_ms = config.initial_delay.as_millis() as f64;
    let max_delay_ms = config.max_delay.as_millis() as f64;

    for attempt in 0..=config.max_retries {
        match f(attempt).await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt == config.max_retries {
                    tracing::error!(
                        "Operation still failed after {} retries: {}",
                        config.max_retries,
                        e
                    );
                    return Err(e);
                }

                let mut current_delay = delay_ms.min(max_delay_ms);
                if config.jitter {
                    let mut rng = rand::rng();
                    current_delay *= 0.5 + rng.random::<f64>();
                }

                tracing::warn!(
                    "Attempt {} failed: {}, retrying in {:.1}s...",
                    attempt + 1,
                    e,
                    current_delay / 1000.0
                );

                tokio::time::sleep(Duration::from_millis(current_delay as u64)).await;
                delay_ms *= config.backoff_factor;
            }
        }
    }

    unreachable!()
}
