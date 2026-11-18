//! A small in-memory lockout for repeated failed sign-ins, per client address.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct LimiterConfig {
    /// Failures inside `window` that trigger a lockout.
    pub max_failures: u32,
    pub window: Duration,
    pub lockout: Duration,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        LimiterConfig { max_failures: 10, window: Duration::from_secs(10 * 60), lockout: Duration::from_secs(15 * 60) }
    }
}

struct Entry {
    failures: u32,
    window_start: Instant,
    locked_until: Option<Instant>,
}

pub struct Limiter {
    config: LimiterConfig,
    entries: Mutex<HashMap<IpAddr, Entry>>,
}

const MAX_TRACKED: usize = 10_000;

impl Limiter {
    pub fn new(config: LimiterConfig) -> Limiter {
        Limiter { config, entries: Mutex::new(HashMap::new()) }
    }

    /// `Err(wait)` while the address is locked out.
    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        self.check_at(ip, Instant::now())
    }

    pub fn failure(&self, ip: IpAddr) {
        self.failure_at(ip, Instant::now());
    }

    pub fn success(&self, ip: IpAddr) {
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).remove(&ip);
    }

    pub fn check_at(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = entries.get_mut(&ip) {
            if let Some(until) = entry.locked_until {
                if until > now {
                    return Err(until - now);
                }
                entries.remove(&ip);
            }
        }
        Ok(())
    }

    pub fn failure_at(&self, ip: IpAddr, now: Instant) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if entries.len() >= MAX_TRACKED {
            let window = self.config.window;
            entries.retain(|_, e| e.locked_until.is_some_and(|u| u > now) || now.duration_since(e.window_start) < window);
        }
        let entry = entries.entry(ip).or_insert(Entry { failures: 0, window_start: now, locked_until: None });
        if now.duration_since(entry.window_start) > self.config.window {
            entry.failures = 0;
            entry.window_start = now;
        }
        entry.failures += 1;
        if entry.failures >= self.config.max_failures {
            entry.locked_until = Some(now + self.config.lockout);
            entry.failures = 0;
            entry.window_start = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    #[test]
    fn locks_after_repeated_failures_and_releases_later() {
        let limiter = Limiter::new(LimiterConfig { max_failures: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(300) });
        let start = Instant::now();
        for _ in 0..2 {
            limiter.failure_at(ip(1), start);
            assert!(limiter.check_at(ip(1), start).is_ok());
        }
        limiter.failure_at(ip(1), start);
        let wait = limiter.check_at(ip(1), start).unwrap_err();
        assert!(wait <= Duration::from_secs(300) && wait > Duration::from_secs(290));
        assert!(limiter.check_at(ip(2), start).is_ok(), "other addresses are unaffected");
        assert!(limiter.check_at(ip(1), start + Duration::from_secs(301)).is_ok());
    }

    #[test]
    fn old_failures_expire_and_success_clears() {
        let limiter = Limiter::new(LimiterConfig { max_failures: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(300) });
        let start = Instant::now();
        limiter.failure_at(ip(1), start);
        limiter.failure_at(ip(1), start);
        limiter.failure_at(ip(1), start + Duration::from_secs(120));
        assert!(limiter.check_at(ip(1), start + Duration::from_secs(120)).is_ok());
        limiter.failure_at(ip(1), start + Duration::from_secs(121));
        limiter.success(ip(1));
        limiter.failure_at(ip(1), start + Duration::from_secs(122));
        assert!(limiter.check_at(ip(1), start + Duration::from_secs(122)).is_ok());
    }
}
