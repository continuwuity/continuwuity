use std::collections::{BTreeMap, BTreeSet};

use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, IdParseError, OwnedEventId, OwnedServerName, OwnedServerSigningKeyId, RoomVersionId, UserId, canonical_json::JsonType, signatures::{JsonError, VerificationError}
};

/// Whether the given event is an `m.room.member` invite that was created as the
/// result of a third-party invite.
///
/// Returns an error if the object has not the expected format of an
/// `m.room.member` event.
pub(super) fn is_invite_via_third_party_id(object: &CanonicalJsonObject) -> Result<bool, JsonError> {
	let Some(CanonicalJsonValue::String(raw_type)) = object.get("type") else {
		return Err(JsonError::NotOfType {
			target: "type".to_owned(),
			of_type: JsonType::String,
		}
		.into());
	};

	if raw_type != "m.room.member" {
		return Ok(false);
	}

	let Some(CanonicalJsonValue::Object(content)) = object.get("content") else {
		return Err(JsonError::NotOfType {
			target: "content".to_owned(),
			of_type: JsonType::Object,
		}
		.into());
	};

	let Some(CanonicalJsonValue::String(membership)) = content.get("membership") else {
		return Err(JsonError::NotOfType {
			target: "membership".to_owned(),
			of_type: JsonType::String,
		}
		.into());
	};

	if membership != "invite" {
		return Ok(false);
	}

	match content.get("third_party_invite") {
		| Some(CanonicalJsonValue::Object(_)) => Ok(true),
		| None => Ok(false),
		| _ => Err(JsonError::NotOfType {
			target: "third_party_invite".to_owned(),
			of_type: JsonType::Object,
		}
		.into()),
	}
}

/// Extracts the server names to check signatures for given event.
///
/// Respects the rules for [validating signatures on received events] for
/// populating the result:
///
/// - Add the server of the sender, except if it's an invite event that results
///   from a third-party invite.
/// - For room versions 1 and 2, add the server of the `event_id`.
/// - For room versions that support restricted join rules, if it's a join event
///   with a `join_authorised_via_users_server`, add the server of that user.
///
/// [validating signatures on received events]: https://spec.matrix.org/latest/server-server-api/#validating-hashes-and-signatures-on-received-events
pub fn servers_to_check_signatures(
	object: &CanonicalJsonObject,
	version: &RoomVersionId,
) -> Result<BTreeSet<OwnedServerName>, VerificationError> {
	let mut servers_to_check = BTreeSet::new();

	if !is_invite_via_third_party_id(object)? {
		match object.get("sender") {
			| Some(CanonicalJsonValue::String(raw_sender)) => {
				let user_id = <&UserId>::try_from(raw_sender.as_str()).map_err(|source| {VerificationError::ParseIdentifier {
						identifier_type: "user ID",
						source,
					}
				})?;

				servers_to_check.insert(user_id.server_name().to_owned());
			},
			| _ =>
				return Err(JsonError::NotOfType {
					target: "sender".to_owned(),
					of_type: JsonType::String,
				}
				.into()),
		};
	}

	match version {
		| RoomVersionId::V1 | RoomVersionId::V2 => match object.get("event_id") {
			| Some(CanonicalJsonValue::String(raw_event_id)) => {
				let event_id: OwnedEventId = raw_event_id.parse().map_err(|source| {
					VerificationError::ParseIdentifier {
						identifier_type: "event ID",
						source,
					}
				})?;

				let server_name = event_id
					.server_name()
					.ok_or_else(|| VerificationError::ParseIdentifier {
                            identifier_type: "event ID",
                            source: IdParseError::InvalidServerName,
                        })?
					.to_owned();

				servers_to_check.insert(server_name);
			},
			| _ => {
				return Err(JsonError::MissingField { path: "event_id".to_owned() }.into());
			},
		},
		| RoomVersionId::V3
		| RoomVersionId::V4
		| RoomVersionId::V5
		| RoomVersionId::V6
		| RoomVersionId::V7 => {},
		// TODO: And for all future versions that have join_authorised_via_users_server
		| RoomVersionId::V8
		| RoomVersionId::V9
		| RoomVersionId::V10
		| RoomVersionId::V11
		| RoomVersionId::V12 => {
			if let Some(authorized_user) = object
				.get("content")
				.and_then(|c| c.as_object())
				.and_then(|c| c.get("join_authorised_via_users_server"))
			{
				let authorized_user = authorized_user.as_str().ok_or_else(|| -> JsonError {
					JsonError::NotOfType {
						target: "join_authorised_via_users_server".to_owned(),
						of_type: JsonType::String,
					}
					.into()
				})?;
				let authorized_user = <&UserId>::try_from(authorized_user)
					.map_err(|source| VerificationError::ParseIdentifier { identifier_type: "user ID", source })?;

				servers_to_check.insert(authorized_user.server_name().to_owned());
			}
		},
		| _ => unimplemented!(),
	}

	Ok(servers_to_check)
}

/// Extracts the server names and key ids to check signatures for given event.
pub fn required_keys(
	object: &CanonicalJsonObject,
	version: &RoomVersionId,
) -> Result<BTreeMap<OwnedServerName, Vec<OwnedServerSigningKeyId>>, VerificationError> {
	use CanonicalJsonValue::Object;
	let mut map = BTreeMap::<OwnedServerName, Vec<OwnedServerSigningKeyId>>::new();
	let Some(Object(signatures)) = object.get("signatures") else {
		return Ok(map);
	};

	for server in servers_to_check_signatures(object, version)? {
		let Some(Object(set)) = signatures.get(server.as_str()) else {
			continue;
		};

		let entry = map.entry(server.clone()).or_default();
		set.iter()
			.map(|(k, _)| k.clone())
			.map(TryInto::try_into)
			.filter_map(Result::ok)
			.for_each(|key_id| entry.push(key_id));
	}

	Ok(map)
}
