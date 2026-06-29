use std::{ops::RangeInclusive, time::Duration};

/// Returns a `Duration` that is jittered by a random percentage of the base
/// duration. The jitter is applied as a random percentage in the range of
/// `-jitter_range` to `jitter_range`.
///
/// # Example
/// ```
/// use conduwuit_core::utils::time::jitter;
/// let sleep_duration = jitter(Duration::from_secs(1), -10..=10);
/// // Adds a jitter of between -10% and 10% to the duration.
/// ```
#[must_use]
pub fn jitter(base: Duration, jitter_range: RangeInclusive<f64>) -> Duration {
	base.mul_f64(1.0 + (rand::random_range(jitter_range)) / 100.0)
}
