/// Compute the delay before the next retry attempt.
///
/// If `retry_after_secs` is Some (from a provider Retry-After header), that value is used
/// directly. Otherwise exponential backoff with ±25% jitter is applied:
///   delay = min(1s × 2^attempt, 64s) × jitter_factor
/// Jitter is derived from subsecond system-time nanoseconds — no external crate required.
pub fn compute_delay(attempt: u32, retry_after_secs: Option<u64>) -> std::time::Duration {
    const BASE_SECS: f64 = 1.0;
    const MAX_SECS: f64 = 64.0;
    const JITTER_AMPLITUDE: f64 = 0.25;

    let secs = if let Some(ra) = retry_after_secs {
        ra as f64
    } else {
        let exp = BASE_SECS * (1u64 << attempt.min(30)) as f64;
        let capped = exp.min(MAX_SECS);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let rand_unit = (nanos % 10_000) as f64 / 10_000.0; // 0.0 .. 1.0
        let jitter = 1.0 - JITTER_AMPLITUDE + 2.0 * JITTER_AMPLITUDE * rand_unit;
        capped * jitter
    };

    std::time::Duration::from_secs_f64(secs.max(0.1))
}

/// Parse the `Retry-After` response header as integer seconds.
/// Returns `None` if the header is absent or not a valid non-negative integer.
/// HTTP-date format is not supported; only integer seconds are parsed.
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Emit a stderr warning if the named rate-limit header reports fewer than 5 requests remaining.
///
/// Pass the provider-specific header name:
///   Anthropic: "anthropic-ratelimit-requests-remaining"
///   OpenAI:    "x-ratelimit-remaining-requests"
///
/// Silently does nothing when the header is absent or unparseable.
pub fn warn_if_quota_low(headers: &reqwest::header::HeaderMap, remaining_header: &str) {
    if let Some(remaining) = headers
        .get(remaining_header)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        if remaining < 5 {
            eprintln!(
                "Warning: AI API rate limit nearly exhausted ({} requests remaining). \
                 Consider waiting before the next run or reducing concurrent usage.",
                remaining
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn compute_delay_first_attempt_within_jitter_bounds() {
        let delay = compute_delay(0, None);
        assert!(
            delay >= Duration::from_millis(750),
            "delay too short: {:?}",
            delay
        );
        assert!(
            delay <= Duration::from_millis(1250),
            "delay too long: {:?}",
            delay
        );
    }

    #[test]
    fn compute_delay_grows_with_attempt() {
        let short = compute_delay(0, None);
        let long = compute_delay(6, None);
        assert!(
            long.as_secs_f64() >= short.as_secs_f64() * 8.0,
            "attempt-6 delay ({:?}) should be at least 8× attempt-0 delay ({:?})",
            long,
            short
        );
    }

    #[test]
    fn compute_delay_caps_at_64_seconds() {
        let delay = compute_delay(20, None);
        let ceiling = Duration::from_secs_f64(64.0 * 1.25 + 0.01);
        assert!(
            delay <= ceiling,
            "delay {:?} exceeds cap ceiling {:?}",
            delay,
            ceiling
        );
    }

    #[test]
    fn compute_delay_retry_after_overrides_backoff() {
        let delay = compute_delay(10, Some(5));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn parse_retry_after_returns_none_when_absent() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_parses_integer_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::HeaderName::from_static("retry-after"),
            reqwest::header::HeaderValue::from_static("30"),
        );
        assert_eq!(parse_retry_after(&headers), Some(30));
    }

    #[test]
    fn parse_retry_after_rejects_http_date() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::HeaderName::from_static("retry-after"),
            reqwest::header::HeaderValue::from_static("Wed, 21 Oct 2025 07:28:00 GMT"),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn warn_if_quota_low_does_not_panic() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-remaining-requests"),
            reqwest::header::HeaderValue::from_static("3"),
        );
        warn_if_quota_low(&headers, "x-ratelimit-remaining-requests");

        let mut headers_ok = reqwest::header::HeaderMap::new();
        headers_ok.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-remaining-requests"),
            reqwest::header::HeaderValue::from_static("100"),
        );
        warn_if_quota_low(&headers_ok, "x-ratelimit-remaining-requests");
    }
}
