use conduwuit::Result;
use ruma::{
	CanonicalJsonObject, room_version_rules::RoomVersionRules, signatures::hash_and_sign_event,
};

impl super::Service {
	/// Signs an arbitrary JSON object. The object is mutated.
	pub fn sign_json(&self, object: &mut CanonicalJsonObject) -> Result {
		use ruma::signatures::sign_json;

		let server_name = self.services.globals.server_name().as_str();
		sign_json(server_name, self.keypair(), object).map_err(Into::into)
	}

	/// Hashes and signs an event JSON object. The object is mutated.
	pub fn hash_and_sign_event(
		&self,
		object: &mut CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result {
		hash_and_sign_event(
			self.services.globals.server_name().as_str(),
			self.keypair(),
			object,
			&room_version_rules.redaction,
		)
		.map_err(Into::into)
	}
}
