use conduwuit::{Err, Result, debug, debug_warn, trace};
use ruma::{
	RoomId, ServerName,
	events::{StateEventType, room::server_acl::RoomServerAclEventContent},
};

impl super::Service {
	/// Returns Ok if the acl allows the server, otherwise Forbidden.
	pub async fn acl_check(&self, server_name: &ServerName, room_id: &RoomId) -> Result {
		let Ok(acl_event_content) = self
			.services
			.state_accessor
			.room_state_get_content(room_id, &StateEventType::RoomServerAcl, "")
			.await
			.map(|c: RoomServerAclEventContent| c)
		else {
			return Ok(());
		};

		if acl_event_content.allow.is_empty() {
			debug_warn!(%room_id, "Ignoring broken ACL event (allow key is empty)");
			return Ok(());
		}

		if acl_event_content.deny.contains(&String::from("*"))
			&& acl_event_content.allow.contains(&String::from("*"))
		{
			debug_warn!(%room_id, "Ignoring broken ACL event (allow key and deny key both contain wildcard \"*\"");
			return Ok(());
		}

		if acl_event_content.is_allowed(server_name) {
			trace!("server {server_name} is allowed by ACL");
			Ok(())
		} else {
			debug!("Server {server_name} was denied by room ACL in {room_id}");
			Err!(Request(Forbidden("Server was denied by room ACL")))
		}
	}
}
