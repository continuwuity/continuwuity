use std::collections::BTreeMap;

use ruma::{
	MilliSecondsSinceUnixEpoch,
	events::{AnyTimelineEvent, room::member::MembershipState},
	serde::Raw,
};
use serde_json::value::{RawValue as RawJsonValue, to_raw_value};

use super::{Pdu, sticky};
use crate::Event;

type Unsigned = BTreeMap<String, Box<RawJsonValue>>;

impl Pdu {
	/// Sets the `unsigned` field of the PDU using a mix of the provided
	/// optional data, and information available in the PDU itself. The
	/// `unsigned` field will always be inserted if it is not attached to the
	/// PDU, however no operations will be performed if `unsigned` is present
	/// but malformed and unparseable.
	///
	/// `age` is always inserted and is calculated based on the time of the
	/// function call. `membership`, `prev_content`, and `redacted_by` are
	/// inserted if not `None`. If `user_id` is `None`, `transaction_id` will be
	/// removed. The sticky event TTL field is always inserted and calculated
	/// (per `age`) if the PDU is sticky.
	///
	/// This function panics if any value cannot be serialised, which should not
	/// happen provided `membership`, `prev_content`, and `redacted_because` are
	/// well-formed.
	pub fn set_unsigned(
		&mut self,
		user_id: Option<&ruma::UserId>,
		membership: Option<MembershipState>,
		prev_content: Option<Box<RawJsonValue>>,
		redacted_because: Option<Raw<AnyTimelineEvent>>,
	) {
		// See: https://spec.matrix.org/v1.19/client-server-api/#definition-clientevent_unsigneddata
		let Some(mut unsigned) = self.unsigned().map_or_else(
			|| Some(Unsigned::new()),
			|u| serde_json::from_str::<Unsigned>(u.get()).ok(),
		) else {
			return;
		};

		let now = MilliSecondsSinceUnixEpoch::now().get();
		let now_i: ruma::Int = now.into();
		unsigned.insert(
			"age".to_owned(),
			to_raw_value(&now_i.saturating_sub(self.origin_server_ts.into())).unwrap(),
		);

		if let Some(membership) = membership {
			unsigned.insert("membership".to_owned(), to_raw_value(&membership).unwrap());
		}

		if let Some(prev_content) = prev_content {
			unsigned.insert(
				"prev_content".to_owned(),
				to_raw_value(&prev_content)
					.expect("prev_content must be a valid JSON value for unsigned"),
			);
		}
		if let Some(redacted_because) = redacted_because {
			unsigned.remove("org.continuwuity.redacted_by");
			unsigned.insert(
				"redacted_because".to_owned(),
				to_raw_value(&redacted_because)
					.expect("redacted_because must be a valid JSON value for unsigned"),
			);
		}

		// Remove transaction_id unless the user is the sender
		if user_id.is_none_or(|u| u != self.sender()) {
			unsigned.remove("transaction_id");
		}

		if let Some(expires_at) = self
			.sticky
			.as_deref()
			.and_then(|sticky| sticky::expires_at(self.origin_server_ts, sticky, u64::from(now)))
		{
			unsigned.insert(
				sticky::TTL_UNSIGNED_KEY.to_owned(),
				to_raw_value(&expires_at.saturating_sub(u64::from(now)))
					.expect("sticky event TTL must be a valid JSON value for unsigned"),
			);
		}

		self.unsigned = Some(to_raw_value(&unsigned).unwrap());
	}
}
