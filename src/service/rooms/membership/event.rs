use conduwuit::{Result, err, utils, utils::TryFutureExtExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedUserId, RoomId, UserId,
	canonical_json::{redact, to_canonical_value},
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	room_version_rules::RoomVersionRules,
};

impl super::Service {
	/// Creates a membership event locally but seeds it with data from a remote
	/// template. This means we retain total control over the created PDU, but
	/// still use the data from the remote server that we have to blindly trust,
	/// like auth/prev events and depth.
	pub(super) async fn seed_local_membership_pdu(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
		membership: MembershipState,
		reason: Option<String>,
		template: CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<CanonicalJsonObject> {
		// First we redact the PDU to remove any unrecognised fields.
		let mut template = redact(template, &room_version_rules.redaction, None)
			.map_err(|e| err!(Request(BadJson("Failed to redact membership template: {e:?}"))))?;

		// Remove fields that are not necessary but not covered by redaction.
		template.remove("event_id");
		template.remove("hashes");
		template.remove("prev_state");
		template.remove("origin");
		template.remove("membership");
		template.remove("redacts");

		// Force set some fields to known good values.
		let join_authorized_via_users_server = {
			if room_version_rules
				.signatures
				.check_join_authorised_via_users_server
			{
				template
					.get("content")
					.map(|s| {
						s.as_object()?
							.get("join_authorised_via_users_server")?
							.as_str()
					})
					.and_then(|s| OwnedUserId::try_from(s.unwrap_or_default()).ok())
			} else {
				None
			}
		};

		let mut content = RoomMemberEventContent::new(membership);
		let (dn, av) = tokio::join!(
			self.services.users.displayname(user_id).ok(),
			self.services.users.avatar_url(user_id).ok()
		);
		content.displayname = dn;
		content.avatar_url = av;
		content.reason = reason;
		content
			.join_authorized_via_users_server
			.clone_from(&join_authorized_via_users_server);
		template.insert("content".to_owned(), to_canonical_value(content)?);
		template.insert(
			"origin_server_ts".to_owned(),
			CanonicalJsonValue::Integer(
				utils::millis_since_unix_epoch()
					.try_into()
					.expect("Timestamp is valid js_int value"),
			),
		);
		template.insert("room_id".to_owned(), room_id.as_str().into());
		template.insert("sender".to_owned(), user_id.to_string().into());
		template.insert("state_key".to_owned(), user_id.to_string().into());
		template.insert("type".to_owned(), StateEventType::RoomMember.to_string().into());

		Ok(template)
	}
}
