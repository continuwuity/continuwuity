use axum::extract::State;
use conduwuit::{
	Err, Event, at,
	utils::{IterStream, ReadyExt, TryFutureExtExt, stream::WidebandExt},
};
use futures::StreamExt;
use ruma::events::room::member::MembershipState;
use ruminuwuity::admin::continuwuity::users::rooms::v1::{Request, Response, RoomInfo};
use serde::Deserialize;

use crate::router::Ruma;

/// # `GET /_continuwuity/admin/v1/users/{user_id}/rooms`
///
/// Lists the memberships of a user on this server.
pub(crate) async fn list_user_rooms(
	State(services): State<crate::State>,
	body: Ruma<Request>,
) -> conduwuit::Result<Response> {
	if !services
		.users
		.is_admin(body.identity.expect_sender_user()?)
		.await
	{
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	let limit = body.limit.unwrap_or(100).try_into()?;
	let offset = body
		.limit
		.unwrap_or(100)
		.saturating_mul(body.page.unwrap_or_default())
		.try_into()?;

	let room_ids: Vec<_> = match body.state {
		| MembershipState::Join =>
			services
				.rooms
				.state_cache
				.rooms_joined(&body.user_id)
				.skip(offset)
				.take(limit)
				.collect()
				.await,
		| MembershipState::Invite =>
			services
				.rooms
				.state_cache
				.rooms_invited(&body.user_id)
				.skip(offset)
				.take(limit)
				.map(at!(0))
				.collect()
				.await,
		| MembershipState::Knock =>
			services
				.rooms
				.state_cache
				.rooms_knocked(&body.user_id)
				.skip(offset)
				.take(limit)
				.map(at!(0))
				.collect()
				.await,
		| MembershipState::Leave =>
			services
				.rooms
				.state_cache
				.rooms_left(&body.user_id)
				.skip(offset)
				.take(limit)
				.ready_filter_map(|(room_id, pdu)| {
					pdu.is_some_and(|pdu| {
						pdu.get_content::<ExtractMembership>()
							.expect("membership PDU must have a membership field")
							.membership
							== MembershipState::Leave
					})
					.then_some(room_id)
				})
				.collect()
				.await,
		| MembershipState::Ban =>
			services
				.rooms
				.state_cache
				.rooms_left(&body.user_id)
				.skip(offset)
				.take(limit)
				.ready_filter_map(|(room_id, pdu)| {
					pdu.is_some_and(|pdu| {
						pdu.get_content::<ExtractMembership>()
							.expect("membership PDU must have a membership field")
							.membership
							== MembershipState::Ban
					})
					.then_some(room_id)
				})
				.collect()
				.await,
		| _ => return Err!(Request(InvalidParam("Unknown membership kind: {:?}", body.state))),
	};

	let resp = room_ids
		.into_iter()
		.stream()
		.wide_then(|room_id| async move {
			let (name, avatar, alias, members) = tokio::join!(
				services.rooms.state_accessor.get_name(&room_id).ok(),
				services.rooms.state_accessor.get_avatar(&room_id),
				services
					.rooms
					.state_accessor
					.get_canonical_alias(&room_id)
					.ok(),
				services.rooms.state_cache.room_members(&room_id).count(),
			);
			(room_id, RoomInfo {
				name,
				avatar_url: avatar.into_option().and_then(|a| a.url),
				canonical_alias: alias,
				member_count: members.try_into().unwrap_or(0),
			})
		})
		.collect()
		.await;

	Ok(Response::new(resp))
}

#[derive(Deserialize)]
struct ExtractMembership {
	pub membership: MembershipState,
}
