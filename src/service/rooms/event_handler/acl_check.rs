use conduwuit::{Err, Result, debug, implement, trace, warn};
use ruma::{
	RoomId, ServerName,
	events::{StateEventType, room::server_acl::RoomServerAclEventContent},
};

/// Returns Ok if the acl allows the server
#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
pub async fn acl_check(&self, server_name: &ServerName, room_id: &RoomId) -> Result {
	let Ok(acl_event_content) = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomServerAcl, "")
		.await
		.map(|c: RoomServerAclEventContent| c)
		.inspect(|acl| trace!(%room_id, "ACL content found: {acl:?}"))
		.inspect_err(|e| trace!(%room_id, "No ACL content found: {e:?}"))
	else {
		return Ok(());
	};

	if acl_event_content.allow.is_empty() {
		warn!(%room_id, "Ignoring broken ACL event (allow key is empty)");
		return Ok(());
	}

	if acl_event_content.deny.contains(&String::from("*"))
		&& acl_event_content.allow.contains(&String::from("*"))
	{
		warn!(%room_id, "Ignoring broken ACL event (allow key and deny key both contain wildcard \"*\"");
		return Ok(());
	}

	if acl_event_content.is_allowed(server_name) {
		trace!("server {server_name} is allowed by ACL");
		Ok(())
	} else {
		if acl_event_content.deny.contains(&String::from("*"))
			&& server_name == self.services.globals.server_name()
		{
			warn!(%room_id, "Ignoring broken ACL event that denies everyone");
			return Ok(());
		}
		debug!("Server {server_name} was denied by room ACL in {room_id}");
		Err!(Request(Forbidden("Server was denied by room ACL")))
	}
}
