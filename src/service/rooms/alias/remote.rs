use conduwuit::{Result, debug, error, implement};
use federation::query::get_room_information::v1::Response;
use ruma::{OwnedRoomId, OwnedServerName, RoomAliasId, ServerName, api::federation};

#[implement(super::Service)]
pub(super) async fn remote_resolve(
	&self,
	room_alias: &RoomAliasId,
) -> Result<(OwnedRoomId, Vec<OwnedServerName>)> {
	debug!("Asking {} to resolve {room_alias:?}", room_alias.server_name());
	match self
		.remote_request(room_alias, room_alias.server_name())
		.await
	{
		| Err(e) => {
			error!("Unable to resolve remote room alias {}: {e}", room_alias);
			Err(e)
		},
		| Ok(Response { room_id, servers }) => {
			debug!("Remote resolved {room_alias:?} to {room_id:?} with servers {servers:?}");
			Ok((room_id, servers))
		},
	}
}

#[implement(super::Service)]
async fn remote_request(
	&self,
	room_alias: &RoomAliasId,
	server: &ServerName,
) -> Result<Response> {
	use federation::query::get_room_information::v1::Request;

	let request = Request { room_alias: room_alias.to_owned() };

	self.services
		.sending
		.send_federation_request(server, request)
		.await
}
