use std::{cmp, time::Duration};

/// Returns false if the exponential backoff has expired based on the inputs
#[inline]
#[must_use]
pub fn continue_exponential_backoff_secs(
	min: u64,
	max: u64,
	elapsed: Duration,
	tries: u32,
) -> bool {
	let min = Duration::from_secs(min);
	let max = Duration::from_secs(max);
	continue_exponential_backoff(min, max, elapsed, tries)
}

/// Returns false if the exponential backoff has expired based on the inputs
#[inline]
#[must_use]
pub fn continue_exponential_backoff(
	min: Duration,
	max: Duration,
	elapsed: Duration,
	tries: u32,
) -> bool {
	let min = min.saturating_mul(tries).saturating_mul(tries);
	let min = cmp::min(min, max);
	elapsed < min
}

/// Determines the minimum number of backoff seconds
#[must_use]
pub fn min_exp_backoff_duration(min: u64, max: u64, retries: u32) -> Duration {
	let min = Duration::from_secs(min)
		.saturating_mul(retries)
		.saturating_mul(retries);
	Duration::from_secs(max).min(min)
}
