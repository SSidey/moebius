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
#[path = "retry_tests.rs"]
mod tests;
