//! Sticky event helpers, as defined by [MSC4354].
//!
//! [MSC4354]: https://github.com/matrix-org/matrix-spec-proposals/pull/4354

use ruma::{UInt, events::sticky::StickyDurationMs};
use serde::Deserialize;
use serde_json::{
	json,
	value::{RawValue as RawJsonValue, to_raw_value},
};

/// Name of the sticky object at the top level of a PDU. Unstable prefix.
pub const PDU_KEY: &str = "msc4354_sticky";

/// Name of the expiry hint injected into `unsigned`. Unstable prefix.
pub const TTL_UNSIGNED_KEY: &str = "msc4354_sticky_duration_ttl_ms";

/// The longest an event may remain sticky (one hour).
#[allow(clippy::as_conversions)]
pub const MAX_DURATION_MS: u64 = StickyDurationMs::MAX as u64;

#[derive(Deserialize)]
struct Object {
	duration_ms: u64,
}

/// Build the sticky object for an event we are creating locally.
#[must_use]
pub fn object(duration_ms: StickyDurationMs) -> Box<RawJsonValue> {
	to_raw_value(&json!({ "duration_ms": duration_ms }))
		.expect("a StickyDurationMs always produces a valid sticky object")
}

/// The sticky duration of `sticky`, clamped to [`MAX_DURATION_MS`], or `None`
/// if this is not a valid sticky object.
///
/// A malformed object leaves the event unsticky rather than invalidating it, so
/// a peer more lenient than us cannot split the room DAG.
#[must_use]
pub fn duration_ms(sticky: &RawJsonValue) -> Option<u64> {
	serde_json::from_str::<Object>(sticky.get())
		.ok()
		.map(|sticky| sticky.duration_ms.min(MAX_DURATION_MS))
}

/// The time at which an event stops being sticky, in milliseconds since the
/// unix epoch, or `None` if the event was never sticky.
///
/// MSC4354 starts the clock at `min(received_ts, origin_server_ts)` so a server
/// cannot extend stickiness by dating its events forward. We keep no receive
/// time per PDU, so instead we deny stickiness to events dated further ahead
/// than any event could still be sticky for.
#[must_use]
pub fn expires_at(origin_server_ts: UInt, sticky: &RawJsonValue, now: u64) -> Option<u64> {
	let origin_server_ts = u64::from(origin_server_ts);
	if origin_server_ts.saturating_sub(now) > MAX_DURATION_MS {
		return None;
	}

	let start = origin_server_ts.min(now);
	duration_ms(sticky).map(|duration_ms| start.saturating_add(duration_ms))
}

#[must_use]
pub fn is_sticky(origin_server_ts: UInt, sticky: &RawJsonValue, now: u64) -> bool {
	expires_at(origin_server_ts, sticky, now).is_some_and(|expires_at| expires_at > now)
}

#[cfg(test)]
mod tests {
	use ruma::{UInt, events::sticky::StickyDurationMs};
	use serde_json::value::{RawValue as RawJsonValue, to_raw_value};

	use super::{MAX_DURATION_MS, duration_ms, expires_at, is_sticky, object};

	fn raw(json: &serde_json::Value) -> Box<RawJsonValue> { to_raw_value(json).unwrap() }

	fn ts(millis: u64) -> UInt { UInt::try_from(millis).unwrap() }

	#[test]
	fn duration_of_valid_object() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 300_000 }));
		assert_eq!(duration_ms(&sticky), Some(300_000));
	}

	#[test]
	fn duration_is_clamped_to_one_hour() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 9_999_999_999_u64 }));
		assert_eq!(duration_ms(&sticky), Some(MAX_DURATION_MS));
	}

	#[test]
	fn malformed_objects_are_not_sticky() {
		for malformed in [
			serde_json::json!({}),
			serde_json::json!({ "duration_ms": -1 }),
			serde_json::json!({ "duration_ms": "300000" }),
			serde_json::json!({ "duration_ms": 3000.5 }),
			serde_json::json!("nonsense"),
			serde_json::json!(null),
		] {
			let sticky = raw(&malformed);
			assert_eq!(duration_ms(&sticky), None, "{malformed} should not be sticky");
		}
	}

	#[test]
	fn object_round_trips_through_duration_ms() {
		let sticky = object(StickyDurationMs::new_clamped(600_000_u32));
		assert_eq!(duration_ms(&sticky), Some(600_000));
	}

	#[test]
	fn expiry_is_origin_server_ts_plus_duration() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 300_000 }));
		assert_eq!(expires_at(ts(1_000_000), &sticky, 1_100_000), Some(1_300_000));
	}

	#[test]
	fn expiry_uses_the_clamped_duration() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 9_999_999_999_u64 }));
		assert_eq!(
			expires_at(ts(1_000_000), &sticky, 1_000_000),
			Some(1_000_000 + MAX_DURATION_MS)
		);
	}

	#[test]
	fn timestamps_far_in_the_future_are_not_sticky() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 300_000 }));
		let now = 1_000_000;

		// a little ahead of us is tolerated, and starts the clock at `now`
		let slightly_ahead = ts(now + MAX_DURATION_MS);
		assert_eq!(expires_at(slightly_ahead, &sticky, now), Some(now + 300_000));

		// beyond the point any honest event could still be sticky, it is not
		let far_ahead = ts(now + MAX_DURATION_MS + 1);
		assert_eq!(expires_at(far_ahead, &sticky, now), None);
	}

	#[test]
	fn stickiness_ends_at_the_expiry() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 300_000 }));
		assert!(is_sticky(ts(1_000_000), &sticky, 1_299_999));
		assert!(!is_sticky(ts(1_000_000), &sticky, 1_300_000));
		assert!(!is_sticky(ts(1_000_000), &sticky, 1_300_001));
	}

	#[test]
	fn zero_duration_is_never_sticky() {
		let sticky = raw(&serde_json::json!({ "duration_ms": 0 }));
		assert!(!is_sticky(ts(1_000_000), &sticky, 1_000_000));
	}
}
