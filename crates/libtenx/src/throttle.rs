use crate::{
    error::{Result, TenxError},
    events::{send_event, Event, EventSender},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

const BACKOFF_MULTIPLIER: f64 = 2.0;
const MAX_BACKOFF_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum Throttle {
    /// Throttle for a specified number of seconds
    RetryAfter(u64),
    /// Throttle with exponential backoff
    Backoff,
}

impl std::fmt::Display for Throttle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Throttle::RetryAfter(secs) => write!(f, "retry after {secs} seconds"),
            Throttle::Backoff => write!(f, "rate limited"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Throttler {
    retries: u32,
    max_retries: u32,
}

impl Throttler {
    pub fn new(max_retries: u32) -> Self {
        Throttler {
            retries: 0,
            max_retries,
        }
    }

    /// Calculate the time to sleep for throttling and update retry count
    pub fn throttle_time(&mut self, t: &Throttle) -> Result<Duration> {
        if self.retries >= self.max_retries {
            return Err(TenxError::MaxRetries(self.retries as u64));
        }

        Ok(match t {
            Throttle::RetryAfter(seconds) => {
                self.retries = 0;
                Duration::from_secs(*seconds)
            }
            Throttle::Backoff => {
                let backoff =
                    (BACKOFF_MULTIPLIER.powi(self.retries as i32) as u64).min(MAX_BACKOFF_SECS);
                self.retries = self.retries.saturating_add(1);
                Duration::from_secs(backoff)
            }
        })
    }

    /// Reset the retry count to zero
    pub fn reset(&mut self) {
        self.retries = 0;
    }

    /// Throttle by sleeping until we can make the next request.
    pub async fn throttle(&mut self, t: &Throttle, sender: &Option<EventSender>) -> Result<()> {
        let duration = self.throttle_time(t)?;
        send_event(sender, Event::Throttled(duration.as_millis() as u64))?;
        sleep(duration).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_after() {
        let mut throttler = Throttler::new(20);
        let duration = throttler.throttle_time(&Throttle::RetryAfter(10)).unwrap();
        assert_eq!(duration, Duration::from_secs(10));
        assert_eq!(throttler.retries, 0); // Verify retries reset
    }

    #[test]
    fn test_exponential_backoff() {
        let mut throttler = Throttler::new(20);

        // First retry
        let duration = throttler.throttle_time(&Throttle::Backoff).unwrap();
        assert_eq!(duration, Duration::from_secs(1)); // 2.0^0 = 1
        assert_eq!(throttler.retries, 1);

        // Second retry
        let duration = throttler.throttle_time(&Throttle::Backoff).unwrap();
        assert_eq!(duration, Duration::from_secs(2)); // 2.0^1 = 2
        assert_eq!(throttler.retries, 2);

        // Third retry
        let duration = throttler.throttle_time(&Throttle::Backoff).unwrap();
        assert_eq!(duration, Duration::from_secs(4)); // 2.0^2 = 4
        assert_eq!(throttler.retries, 3);
    }

    #[test]
    fn test_backoff_cap() {
        let mut throttler = Throttler::new(20);
        throttler.retries = 10; // High retry count
        let duration = throttler.throttle_time(&Throttle::Backoff).unwrap();
        assert_eq!(duration, Duration::from_secs(MAX_BACKOFF_SECS));
        assert_eq!(throttler.retries, 11);
    }

    #[test]
    fn test_max_retries() {
        let mut throttler = Throttler::new(3);

        // First three retries should work
        assert!(throttler.throttle_time(&Throttle::Backoff).is_ok());
        assert!(throttler.throttle_time(&Throttle::Backoff).is_ok());
        assert!(throttler.throttle_time(&Throttle::Backoff).is_ok());

        // Fourth retry should fail
        match throttler.throttle_time(&Throttle::Backoff) {
            Err(TenxError::MaxRetries(3)) => (),
            other => panic!("Expected MaxRetries error, got {other:?}"),
        }
    }
}
