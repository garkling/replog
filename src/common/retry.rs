use std::env;
use std::cmp::min;
use std::time::Duration;
use lazy_static::lazy_static;

use tokio::time::sleep;
use rand::{Rng, thread_rng};

lazy_static! {
    pub static ref MAX_RETRIES: u16 = env::var("MAX_RETRIES")
        .unwrap_or_default()
        .parse()
        .unwrap_or(5);
    pub static ref INIT_BACKOFF_MS: u64 = env::var("INIT_BACKOFF_MS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(1000);
    pub static ref MAX_BACKOFF_MS: u64  = env::var("MAX_BACKOFF_MS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(3600*1000);
    pub static ref BACKOFF_FACTOR: u64  = env::var("BACKOFF_FACTOR")
        .unwrap_or_default()
        .parse()
        .unwrap_or(2);
}


pub struct Attempts {
    pub n: u16,
    pub backoff_ms: Duration
}

impl Default for Attempts {
    fn default() -> Self {
        Self {
            n: *MAX_RETRIES,
            backoff_ms: Duration::from_millis(*INIT_BACKOFF_MS)
        }
    }
}

impl Attempts {

    pub fn next(&mut self) -> bool {
        if self.n > 0 {
            self.n -= 1;
            let new_backoff = min(
                *MAX_BACKOFF_MS,
                (self.backoff_ms.as_millis() as u64 * *BACKOFF_FACTOR)
                    + self.jitter()
                );

            self.backoff_ms = Duration::from_millis(new_backoff);
            return true
        }

        return false
    }

    fn jitter(&self) -> u64 {
        thread_rng().gen_range(0..self.backoff_ms.as_millis()) as u64
    }

    pub async fn delay(&self) {
        if self.n != 0 {
            sleep(self.backoff_ms).await
        }
    }
}
