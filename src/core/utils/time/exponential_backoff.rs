use std::time::Duration;

/// Returns false if the backoff interval has expired based on the inputs,
/// meaning the operation should be retried.
#[inline]
#[must_use]
pub fn should_continue_backoff(
	min: Duration,
	max: Duration,
	elapsed: Duration,
	tries: u32,
) -> bool {
	elapsed < next_interval(min, max, tries)
}

/// Determines the interval that should be waited before retrying the operation
/// using the algorithm: `(min * retries).min(max)`.
#[must_use]
#[inline]
pub fn next_interval(min: Duration, max: Duration, retries: u32) -> Duration {
	// TODO(nex): jitter?
	min.saturating_mul(retries).min(max)
}
