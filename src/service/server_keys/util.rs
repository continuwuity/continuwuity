use std::collections::BTreeMap;

use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedServerName, OwnedServerSigningKeyId,
	room_version_rules::SignaturesRules,
	signatures::{VerificationError, required_server_signatures_to_verify_event},
};

/// Extracts the server names and key ids to check signatures for given event.
pub(super) fn required_keys(
	object: &CanonicalJsonObject,
	rules: &SignaturesRules,
) -> Result<BTreeMap<OwnedServerName, Vec<OwnedServerSigningKeyId>>, VerificationError> {
	use CanonicalJsonValue::Object;
	let mut map = BTreeMap::<OwnedServerName, Vec<OwnedServerSigningKeyId>>::new();
	let Some(Object(signatures)) = object.get("signatures") else {
		return Ok(map);
	};

	for server in required_server_signatures_to_verify_event(object, rules)? {
		let Some(Object(set)) = signatures.get(server.as_str()) else {
			continue;
		};

		let entry = map.entry(server.clone()).or_default();
		set.keys()
			.cloned()
			.map(TryInto::try_into)
			.filter_map(Result::ok)
			.for_each(|key_id| entry.push(key_id));
	}

	Ok(map)
}
