use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::Local;
use rand::Rng;
use tracing::{debug, warn};

use crate::{
    config::Config,
    error::{KslError, Result},
};

struct State {
    last_request_ms: Option<u64>,
    daily_count: u32,
    daily_count_date: String,
    backoff_until_secs: u64,
    consecutive_failures: u32,
}

#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<State>>,
    min_delay_ms: u64,
    max_delay_ms: u64,
    daily_cap: u32,
    backoff_initial_ms: u64,
    backoff_max_ms: u64,
    state_file: PathBuf,
}

impl RateLimiter {
    pub fn new(config: &Config) -> Self {
        let state_file = config.data_dir.join("rate_state.txt");
        let (daily_count, daily_count_date, backoff_until_secs) =
            Self::load_state(&state_file);

        let state = State {
            last_request_ms: None,
            daily_count,
            daily_count_date,
            backoff_until_secs,
            consecutive_failures: 0,
        };

        RateLimiter {
            state: Arc::new(Mutex::new(state)),
            min_delay_ms: config.min_delay_ms,
            max_delay_ms: config.max_delay_ms,
            daily_cap: config.daily_request_cap,
            backoff_initial_ms: 30_000,
            backoff_max_ms: 300_000,
            state_file,
        }
    }

    pub async fn acquire(&self) -> Result<()> {
        let current_ms = now_ms();
        let today = Local::now().format("%Y-%m-%d").to_string();

        let sleep_ms = {
            let mut s = self.state.lock().unwrap();

            // Date rollover
            if s.daily_count_date != today {
                s.daily_count = 0;
                s.daily_count_date = today.clone();
            }

            // Cap check
            if s.daily_count >= self.daily_cap {
                return Err(KslError::DailyCapExceeded { cap: self.daily_cap });
            }

            // Backoff check
            let now_secs = current_ms / 1000;
            if s.backoff_until_secs > now_secs {
                let wait_secs = s.backoff_until_secs - now_secs;
                return Err(KslError::RateLimited {
                    reason: format!("backoff active for {}s more", wait_secs),
                });
            }

            // Compute delay
            let elapsed = s.last_request_ms.map(|t| current_ms.saturating_sub(t)).unwrap_or(u64::MAX);
            let mut rng = rand::thread_rng();
            let target = rng.gen_range(self.min_delay_ms..=self.max_delay_ms);
            target.saturating_sub(elapsed)
        };
        // Mutex released before sleep
        if sleep_ms > 0 {
            debug!("rate limiter sleeping {}ms", sleep_ms);
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }

        // Re-acquire to update state
        {
            let mut s = self.state.lock().unwrap();
            s.last_request_ms = Some(now_ms());
            s.daily_count += 1;
        }

        self.persist();
        Ok(())
    }

    pub fn record_success(&self) {
        let mut s = self.state.lock().unwrap();
        s.consecutive_failures = 0;
        s.backoff_until_secs = 0;
    }

    pub fn record_failure(&self) {
        let mut s = self.state.lock().unwrap();
        s.consecutive_failures += 1;
        let base = self.backoff_initial_ms * (1u64 << (s.consecutive_failures - 1).min(10));
        let capped = base.min(self.backoff_max_ms);
        // ±20% jitter
        let mut rng = rand::thread_rng();
        let jitter = rng.gen_range(0..=(capped / 5));
        let backoff_ms = if rng.gen_bool(0.5) {
            capped + jitter
        } else {
            capped.saturating_sub(jitter)
        };
        let until = now_ms() / 1000 + backoff_ms / 1000;
        s.backoff_until_secs = until;
        warn!(
            consecutive_failures = s.consecutive_failures,
            backoff_secs = backoff_ms / 1000,
            "429 received, backing off"
        );
    }

    fn load_state(path: &PathBuf) -> (u32, String, u64) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (0, today, 0),
            Err(e) => {
                warn!("could not read rate state file: {}; assuming cap reached", e);
                return (0, today, 0);
            }
        };
        let mut lines = content.lines();
        let date = lines.next().unwrap_or("").to_string();
        let count: u32 = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);
        let backoff: u64 = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);

        let count = if date == today { count } else { 0 };
        let date = if date == today { date } else { today };
        (count, date, backoff)
    }

    fn persist(&self) {
        let (count, date, backoff) = {
            let s = self.state.lock().unwrap();
            (s.daily_count, s.daily_count_date.clone(), s.backoff_until_secs)
        };
        let content = format!("{}\n{}\n{}\n", date, count, backoff);
        if let Some(parent) = self.state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&self.state_file, content) {
            warn!("Failed to persist rate limiter state: {}", e);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
