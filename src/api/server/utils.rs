use conduwuit::{Err, Result, implement, is_false};
use conduwuit_service::Services;
use futures::{FutureExt, future::OptionFuture, join};
use ruma::{EventId, RoomId, ServerName};

pub(super) struct AccessCheck<'a> {
	pub(super) services: &'a Services,
	pub(super) origin: &'a ServerName,
	pub(super) room_id: &'a RoomId,
	pub(super) event_id: Option<&'a EventId>,
}

#[implement(AccessCheck, params = "<'_>")]
pub(super) async fn check(&self) -> Result {
	let acl_check = self
		.services
		.rooms
		.event_handler
		.acl_check(self.origin, self.room_id)
		.map(|result| result.is_ok());

	let world_readable = self
		.services
		.rooms
		.state_accessor
		.is_world_readable(self.room_id);

	let server_in_room = self
		.services
		.rooms
		.state_cache
		.server_in_room(self.origin, self.room_id);

	let server_can_see: OptionFuture<_> = self
		.event_id
		.map(|event_id| {
			self.services.rooms.state_accessor.server_can_see_event(
				self.origin,
				self.room_id,
				event_id,
			)
		})
		.into();

	let (world_readable, server_in_room, server_can_see, acl_check) =
		join!(world_readable, server_in_room, server_can_see, acl_check);

	if !acl_check {
		return Err!(Request(Forbidden("Server access denied.")));
	}

	if !world_readable && !server_in_room {
		return Err!(Request(Forbidden("Server is not in room.")));
	}

	if server_can_see.is_some_and(is_false!()) {
		return Err!(Request(Forbidden("Server is not allowed to see event.")));
	}

	Ok(())
}
