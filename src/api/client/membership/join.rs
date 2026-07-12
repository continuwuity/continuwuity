use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{
	Err, Result, debug,
	result::FlatOk,
	utils::{shuffle, stream::IterStream},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedRoomId, OwnedServerName, OwnedUserId, UserId,
	api::client::membership::{join_room_by_id, join_room_by_id_or_alias},
};

use super::banned_room_check;
use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/join`
///
/// Tries to join the sender user into a room.
///
/// - If the server knowns about this room: creates the join event and does auth
///   rules locally
/// - If the server does not know about the room: asks other servers over
///   federation
#[tracing::instrument(skip_all, fields(%client), name = "join", level = "info")]
pub(crate) async fn join_room_by_id_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<join_room_by_id::v3::Request>,
) -> Result<join_room_by_id::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	banned_room_check(
		&services,
		sender_user,
		Some(&body.room_id),
		body.room_id.server_name(),
		client,
	)
	.await?;

	// There is no body.server_name for /roomId/join
	let mut servers: Vec<_> = services
		.rooms
		.state_cache
		.servers_invite_via(&body.room_id)
		.collect()
		.await;

	servers.extend(
		services
			.rooms
			.state_cache
			.invite_state(sender_user, &body.room_id)
			.await
			.unwrap_or_default()
			.iter()
			.filter_map(|event| event.get_field("sender").ok().flatten())
			.filter_map(|sender: &str| UserId::parse(sender).ok())
			.map(|user| user.server_name().to_owned()),
	);

	if let Some(server) = body.room_id.server_name() {
		servers.push(server.into());
	}

	servers.sort_unstable();
	servers.dedup();
	shuffle(&mut servers);
	let servers = deprioritize(servers, &services.config.deprioritize_joins_through_servers);

	let room_id = services
		.rooms
		.membership
		.join_room(sender_user, &body.room_id, body.reason.clone(), servers)
		.boxed()
		.await?;

	Ok(join_room_by_id::v3::Response::new(room_id))
}

/// # `POST /_matrix/client/r0/join/{roomIdOrAlias}`
///
/// Tries to join the sender user into a room.
///
/// - If the server knowns about this room: creates the join event and does auth
///   rules locally
/// - If the server does not know about the room: use the server name query
///   param if specified. if not specified, asks other servers over federation
///   via room alias server name and room ID server name
#[tracing::instrument(skip_all, fields(%client), name = "join", level = "info")]
pub(crate) async fn join_room_by_id_or_alias_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<join_room_by_id_or_alias::v3::Request>,
) -> Result<join_room_by_id_or_alias::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let body = &body.body;
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let (servers, room_id) = match OwnedRoomId::try_from(body.room_id_or_alias.clone()) {
		| Ok(room_id) => {
			banned_room_check(
				&services,
				sender_user,
				Some(&room_id),
				room_id.server_name(),
				client,
			)
			.boxed()
			.await?;

			let mut servers = body.via.clone();
			if servers.is_empty() {
				debug!("No via servers provided for join, injecting some.");
				servers.extend(
					services
						.rooms
						.state_cache
						.servers_invite_via(&room_id)
						.collect::<Vec<_>>()
						.await,
				);

				servers.extend(
					services
						.rooms
						.state_cache
						.invite_state(sender_user, &room_id)
						.await
						.unwrap_or_default()
						.iter()
						.filter_map(|event| event.get_field("sender").ok().flatten())
						.filter_map(|sender: &str| UserId::parse(sender).ok())
						.map(|user| user.server_name().to_owned()),
				);

				if let Some(server) = room_id.server_name() {
					servers.push(server.to_owned());
				}
			}

			servers.sort_unstable();
			servers.dedup();
			shuffle(&mut servers);

			(servers, room_id)
		},
		| Err(room_alias) => {
			let (room_id, mut servers) = services.rooms.alias.resolve_alias(&room_alias).await?;

			banned_room_check(
				&services,
				sender_user,
				Some(&room_id),
				Some(room_alias.server_name()),
				client,
			)
			.await?;

			let addl_via_servers = services.rooms.state_cache.servers_invite_via(&room_id);

			let addl_state_servers = services
				.rooms
				.state_cache
				.invite_state(sender_user, &room_id)
				.await
				.unwrap_or_default();

			let mut addl_servers: Vec<_> = addl_state_servers
				.iter()
				.map(|event| event.get_field("sender"))
				.filter_map(FlatOk::flat_ok)
				.map(|user: OwnedUserId| user.server_name().to_owned())
				.stream()
				.chain(addl_via_servers)
				.collect()
				.await;

			addl_servers.sort_unstable();
			addl_servers.dedup();
			shuffle(&mut addl_servers);
			servers.append(&mut addl_servers);

			(servers, room_id)
		},
	};

	let servers = deprioritize(servers, &services.config.deprioritize_joins_through_servers);
	let room_id = services
		.rooms
		.membership
		.join_room(sender_user, &room_id, body.reason.clone(), servers)
		.boxed()
		.await?;

	Ok(join_room_by_id_or_alias::v3::Response::new(room_id))
}

/// Moves deprioritized servers (if any) to the back of the list.
///
/// No-op if we aren't given any servers to deprioritize.
fn deprioritize(
	servers: Vec<OwnedServerName>,
	deprioritized: &[OwnedServerName],
) -> Vec<OwnedServerName> {
	if deprioritized.is_empty() {
		return servers;
	}

	let (mut depr, mut servers): (Vec<_>, Vec<_>) =
		servers.into_iter().partition(|s| deprioritized.contains(s));
	servers.append(&mut depr);
	servers
}

#[cfg(test)]
mod tests {
	use ruma::OwnedServerName;

	use super::*;

	#[test]
	fn deprioritizing_servers_works() -> Result<(), Box<dyn std::error::Error>> {
		let servers = vec![
			"example.com".try_into()?,
			"slow.invalid".try_into()?,
			"example.org".try_into()?,
		];
		let depr = vec!["slow.invalid".try_into()?];
		let expected: Vec<OwnedServerName> = vec![
			"example.com".try_into()?,
			"example.org".try_into()?,
			"slow.invalid".try_into()?,
		];

		let servers = deprioritize(servers, &depr);
		assert_eq!(servers, expected);
		Ok(())
	}

	#[test]
	fn empty_deprioritized_is_noop() -> Result<(), Box<dyn std::error::Error>> {
		let servers = vec![
			"example.com".try_into()?,
			"slow.invalid".try_into()?,
			"example.org".try_into()?,
		];

		let depr_servers = deprioritize(servers.clone(), &[]);
		assert_eq!(depr_servers, servers);
		Ok(())
	}
}
