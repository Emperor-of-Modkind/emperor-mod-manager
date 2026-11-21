//! Shared rate-limit, cooldown, and adaptive pacing coordinator for mod
//! downloads (Nexus API/CDN, Thunderstore, mod.io).

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use reqwest::StatusCode;

/// Maximum `Retry-After` honor window so a bad/huge header doesn't hang forever.
const MAX_RETRY_AFTER_SECS: u64 = 120;

/// Consecutive healthy transfers required before restoring full concurrency.
const HEALTHY_STREAK_TO_RECOVER: u32 = 5;

/// Shared coordinator tracking API quota headers, active cooldowns, and
/// throttle streaks across concurrent or sequential downloads.
#[derive(Debug)]
pub struct RateLimitGate {
    cooldown_until: Mutex<Option<Instant>>,
    hourly_remaining: AtomicI64,
    daily_remaining: AtomicI64,
    throttled_streak: AtomicU32,
    healthy_streak: AtomicU32,
}

impl Default for RateLimitGate {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitGate {
    pub fn new() -> Self {
        Self {
            cooldown_until: Mutex::new(None),
            hourly_remaining: AtomicI64::new(-1),
            daily_remaining: AtomicI64::new(-1),
            throttled_streak: AtomicU32::new(0),
            healthy_streak: AtomicU32::new(0),
        }
    }

    /// Updates Nexus rate-limit quota counters from response headers.
    pub fn observe_nexus_headers(&self, headers: &HeaderMap) {
        if let Some(n) = parse_header_i64(headers, "x-rl-hourly-remaining") {
            let prev = self.hourly_remaining.swap(n, Ordering::Relaxed);
            if n < 25 && (prev < 0 || prev >= 25) {
                log::warn!("Nexus hourly rate limit low: {n} remaining");
            }
        }
        if let Some(n) = parse_header_i64(headers, "x-rl-daily-remaining") {
            let prev = self.daily_remaining.swap(n, Ordering::Relaxed);
            if n < 50 && (prev < 0 || prev >= 50) {
                log::warn!("Nexus daily rate limit low: {n} remaining");
            }
        }
    }

    /// Returns the last observed hourly remaining count, if any.
    pub fn hourly_remaining(&self) -> Option<i64> {
        let v = self.hourly_remaining.load(Ordering::Relaxed);
        (v >= 0).then_some(v)
    }

    /// Records a rate-limit or CDN throttle event and sets a global cooldown
    /// at least as long as `duration`.
    pub fn record_throttle(&self, duration: Duration, reason: &str) {
        let streak = self.throttled_streak.fetch_add(1, Ordering::Relaxed) + 1;
        self.healthy_streak.store(0, Ordering::Relaxed);
        let until = Instant::now() + duration;
        if let Ok(mut guard) = self.cooldown_until.lock() {
            let replace = match *guard {
                Some(existing) => until > existing,
                None => true,
            };
            if replace {
                *guard = Some(until);
            }
        }
        log::warn!(
            "Rate-limit/throttle recorded (streak={streak}, cooldown={}ms): {reason}",
            duration.as_millis()
        );
    }

    /// Records a healthy, unthrottled transfer so concurrency can recover.
    pub fn record_healthy(&self) {
        let healthy = self.healthy_streak.fetch_add(1, Ordering::Relaxed) + 1;
        if healthy >= HEALTHY_STREAK_TO_RECOVER {
            let prev = self.throttled_streak.swap(0, Ordering::Relaxed);
            if prev > 0 {
                log::info!(
                    "Cleared throttle state after {healthy} consecutive healthy transfers"
                );
            }
        }
    }

    /// Returns the recommended concurrent download limit (1 when throttled or
    /// quota is low, otherwise `max_concurrent`).
    pub fn effective_concurrency(&self, max_concurrent: usize) -> usize {
        let max_concurrent = max_concurrent.max(1);
        if self.throttled_streak.load(Ordering::Relaxed) > 0 {
            return 1;
        }
        if let Some(rem) = self.hourly_remaining() {
            if rem < 50 {
                return 1;
            }
        }
        max_concurrent
    }

    /// Sleeps until any active global cooldown expires, returning `false` if
    /// cancelled/paused while waiting.
    pub async fn wait_if_cooled_down(
        &self,
        cancel: Option<&AtomicBool>,
        pause: Option<&AtomicBool>,
    ) -> bool {
        let target = self
            .cooldown_until
            .lock()
            .ok()
            .and_then(|g| *g);
        let Some(until) = target else {
            return !is_stopped(cancel, pause);
        };
        let now = Instant::now();
        if until <= now {
            return !is_stopped(cancel, pause);
        }
        sleep_interruptible(until - now, cancel, pause).await
    }

    /// Applies jittered inter-item pacing during batch modpack/collection
    /// installs. No delay is applied before item 0. Returns `false` if
    /// cancelled/paused while waiting.
    pub async fn pace_batch_item(
        &self,
        download_index: usize,
        cancel: Option<&AtomicBool>,
        pause: Option<&AtomicBool>,
    ) -> bool {
        if !self.wait_if_cooled_down(cancel, pause).await {
            return false;
        }
        if download_index == 0 {
            return true;
        }
        let delay = self.batch_item_delay(download_index);
        if delay.is_zero() {
            return !is_stopped(cancel, pause);
        }
        sleep_interruptible(delay, cancel, pause).await
    }

    fn batch_item_delay(&self, seed: usize) -> Duration {
        let throttled = self.throttled_streak.load(Ordering::Relaxed) > 0;
        let hourly = self.hourly_remaining();
        let (min_ms, max_ms) = if let Some(rem) = hourly {
            if rem < 25 {
                (4000u64, 8000u64)
            } else if rem < 100 || throttled {
                (1500u64, 3000u64)
            } else {
                (250u64, 600u64)
            }
        } else if throttled {
            (1500u64, 3000u64)
        } else {
            (250u64, 600u64)
        };
        Duration::from_millis(jittered_range(min_ms, max_ms, seed as u64))
    }
}

/// Returns `true` if either `cancel` or `pause` flag is set.
pub fn is_stopped(cancel: Option<&AtomicBool>, pause: Option<&AtomicBool>) -> bool {
    cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false)
        || pause.map(|p| p.load(Ordering::SeqCst)).unwrap_or(false)
}

/// Sleeps for `duration` in short increments so cancellation/pause is
/// noticed within ~200ms. Returns `false` if stopped early.
pub async fn sleep_interruptible(
    duration: Duration,
    cancel: Option<&AtomicBool>,
    pause: Option<&AtomicBool>,
) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if is_stopped(cancel, pause) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let step = (deadline - now).min(Duration::from_millis(200));
        tokio::time::sleep(step).await;
    }
}

/// Parses an integer-second `Retry-After` header, clamped to [`MAX_RETRY_AFTER_SECS`].
pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    let secs: u64 = raw.parse().ok()?;
    Some(Duration::from_secs(secs.clamp(1, MAX_RETRY_AFTER_SECS)))
}

/// Returns `true` for HTTP status codes worth retrying with backoff.
pub fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::REQUEST_TIMEOUT
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

/// Computes exponential backoff with jitter: `min(base_ms * 2^attempt, max_ms)`
/// jittered in `[0.75x, 1.25x]`.
pub fn backoff_with_jitter(attempt: u32, base_ms: u64, max_ms: u64) -> Duration {
    let exp = 1u64.checked_shl(attempt.min(10)).unwrap_or(u64::MAX);
    let raw = base_ms.saturating_mul(exp).min(max_ms);
    let min_ms = (raw * 3) / 4;
    let span_max = ((raw * 5) / 4).max(min_ms + 1);
    Duration::from_millis(jittered_range(min_ms, span_max, attempt as u64))
}

fn parse_header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn jittered_range(min_ms: u64, max_ms: u64, salt: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    // Simple splitmix-style mixing so rapid calls still vary.
    let mut x = nanos ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = x ^ (x >> 27);
    let span = max_ms - min_ms + 1;
    min_ms + (x % span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn parses_and_clamps_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("15"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(15)));

        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("9999"));
        assert_eq!(
            parse_retry_after(&headers),
            Some(Duration::from_secs(MAX_RETRY_AFTER_SECS))
        );

        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("not-a-num"));
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn concurrency_scales_down_on_throttle_and_recovers() {
        let gate = RateLimitGate::new();
        assert_eq!(gate.effective_concurrency(3), 3);

        gate.record_throttle(Duration::from_millis(10), "429");
        assert_eq!(gate.effective_concurrency(3), 1);

        for _ in 0..(HEALTHY_STREAK_TO_RECOVER - 1) {
            gate.record_healthy();
            assert_eq!(gate.effective_concurrency(3), 1);
        }
        gate.record_healthy();
        assert_eq!(gate.effective_concurrency(3), 3);
    }

    #[test]
    fn low_hourly_quota_reduces_concurrency() {
        let gate = RateLimitGate::new();
        let mut headers = HeaderMap::new();
        headers.insert("x-rl-hourly-remaining", HeaderValue::from_static("40"));
        gate.observe_nexus_headers(&headers);
        assert_eq!(gate.hourly_remaining(), Some(40));
        assert_eq!(gate.effective_concurrency(3), 1);

        headers.insert("x-rl-hourly-remaining", HeaderValue::from_static("200"));
        gate.observe_nexus_headers(&headers);
        assert_eq!(gate.effective_concurrency(3), 3);
    }

    #[tokio::test]
    async fn pace_aborts_immediately_on_cancel() {
        let gate = RateLimitGate::new();
        gate.record_throttle(Duration::from_secs(30), "test");
        let cancel = AtomicBool::new(true);
        let started = Instant::now();
        let ok = gate.pace_batch_item(1, Some(&cancel), None).await;
        assert!(!ok);
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
