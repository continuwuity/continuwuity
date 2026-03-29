use axum::extract::State;
use conduwuit::{
	Err, Event, Result, at,
	utils::{
		future::TryExtExt,
		stream::{BroadbandExt, ReadyExt},
	},
};
use futures::{FutureExt, StreamExt, future::join};
use ruma::{
	api::client::membership::{
		get_member_events::{self},
		joined_members::{self, v3::RoomMember},
	},
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};

use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/members`
///
/// Lists all joined users in a room (TODO: at a specific point in time, with a
/// specific membership).
///
/// - Only works if the user is currently joined
pub(crate) async fn get_member_events_route(
	State(services): State<crate::State>,
	body: Ruma<get_member_events::v3::Request>,
) -> Result<get_member_events::v3::Response> {
	let sender_user = body.sender_user();
	let membership = body.membership.as_ref();
	let not_membership = body.not_membership.as_ref();

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(sender_user, &body.room_id)
		.await
	{
		return Err!(Request(Forbidden("You don't have permission to view this room.")));
	}

	let chunk = services
			.rooms
			.state_accessor
			.room_state_full(&body.room_id)
			.ready_filter_map(Result::ok)
			.ready_filter(|((ty, _), _)| *ty == StateEventType::RoomMember)
			.map(at!(1))
			.ready_filter_map(|pdu| membership_filter(pdu, membership, not_membership))
			.map(Event::into_format)
			.collect()
			.boxed()
			.await;

	Ok(get_member_events::v3::Response::new(chunk))
}

/// # `POST /_matrix/client/r0/rooms/{roomId}/joined_members`
///
/// Lists all members of a room.
///
/// - The sender user must be in the room
/// - TODO: An appservice just needs a puppet joined
pub(crate) async fn joined_members_route(
	State(services): State<crate::State>,
	body: Ruma<joined_members::v3::Request>,
) -> Result<joined_members::v3::Response> {
	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(body.sender_user(), &body.room_id)
		.await
	{
		return Err!(Request(Forbidden("You don't have permission to view this room.")));
	}

	let joined = services
			.rooms
			.state_cache
			.room_members(&body.room_id)
			.broad_then(|user_id| async move {
				let mut member = RoomMember::new();
				let (display_name, avatar_url) = join(
					services.users.displayname(&user_id).ok(),
					services.users.avatar_url(&user_id).ok(),
				)
				.await;
				member.display_name = display_name;
				member.avatar_url = avatar_url;

				(user_id, member)
			})
			.collect()
			.await;

	Ok(joined_members::v3::Response::new(joined))
}

fn membership_filter<Pdu: Event>(
	pdu: Pdu,
	membership_state_filter: Option<&MembershipState>,
	not_membership_state_filter: Option<&MembershipState>,
) -> Option<impl Event> {
	let evt_membership = pdu.get_content::<RoomMemberEventContent>().ok()?.membership;

	if let Some(membership_state_filter) = membership_state_filter
		&& *membership_state_filter != evt_membership {
		return None;
	}

	if let Some(not_membership_state_filter) = not_membership_state_filter
		&& *not_membership_state_filter == evt_membership {
		return None;
	}

	return Some(pdu);
}
