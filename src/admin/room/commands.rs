use conduwuit::{Err, Event, Result, err};
use futures::StreamExt;
#[allow(unused_imports)]
use ruma::state_res::Event as _;
use ruma::{OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId};

use crate::{PAGE_SIZE, get_room_info};

impl crate::Context<'_> {
	#[allow(clippy::fn_params_excessive_bools)]
	pub(super) async fn list_rooms(
		&self,
		page: Option<usize>,
		exclude_disabled: bool,
		exclude_banned: bool,
		include_empty: bool,
		no_details: bool,
	) -> Result {
		// TODO: i know there's a way to do this with clap, but i can't seem to
		// find it
		let page = page.unwrap_or(1);
		let mut rooms = self
			.services
			.rooms
			.metadata
			.iter_ids()
			.filter_map(|room_id| async move {
				(!exclude_disabled || !self.services.rooms.metadata.is_disabled(&room_id).await)
					.then_some(room_id)
			})
			.filter_map(|room_id| async move {
				(!exclude_banned || !self.services.rooms.metadata.is_banned(&room_id).await)
					.then_some(room_id)
			})
			.then(async |room_id| get_room_info(self.services, &room_id).await)
			.then(|(room_id, total_members, name)| async move {
				let local_members: Vec<_> = self
					.services
					.rooms
					.state_cache
					.active_local_users_in_room(&room_id)
					.collect()
					.await;
				let local_members = local_members.len();
				(room_id, total_members, local_members, name)
			})
			.filter_map(|(room_id, total_members, local_members, name)| async move {
				(include_empty || local_members > 0).then_some((room_id, total_members, name))
			})
			.collect::<Vec<_>>()
			.await;

		rooms.sort_by_key(|r| r.1);
		rooms.reverse();

		let rooms = rooms
			.into_iter()
			.skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
			.take(PAGE_SIZE)
			.collect::<Vec<_>>();

		if rooms.is_empty() {
			return Err!("No more rooms.");
		}

		let body = rooms
			.iter()
			.map(|(id, members, name)| {
				if no_details {
					format!("{id}")
				} else {
					format!("{id}\tMembers: {members}\tName: {name}")
				}
			})
			.collect::<Vec<_>>()
			.join("\n");

		self.write_str(&format!("Rooms ({}):\n```\n{body}\n```", rooms.len()))
			.await
	}

	pub(super) async fn exists(&self, room_id: OwnedRoomId) -> Result {
		let result = self.services.rooms.metadata.exists(&room_id).await;

		self.write_str(&format!("{result}")).await
	}

	pub(super) async fn export_state(
		&self,
		room: OwnedRoomOrAliasId,
		event_id: Option<OwnedEventId>,
	) -> Result {
		self.bail_restricted()?;

		let room_id = self.services.rooms.alias.resolve(&room).await?;
		let shortstatehash = match &event_id {
			| Some(event_id) =>
				self.services
					.rooms
					.state_accessor
					.pdu_shortstatehash(event_id)
					.await?,
			| None =>
				self.services
					.rooms
					.state
					.get_room_shortstatehash(&room_id)
					.await?,
		};
		let mut state_at: Vec<_> = self
			.services
			.rooms
			.state_accessor
			.state_full_pdus(shortstatehash)
			.map(Event::into_pdu)
			.collect()
			.await;

		if let Some(event_id) = event_id {
			let pdu = self.services.rooms.timeline.get_pdu(&event_id).await?;
			if let Some(state_key) = pdu.state_key() {
				state_at.retain(|p| p.kind() != pdu.kind() || p.state_key() != Some(state_key));
				state_at.push(pdu);
			}
		}

		if state_at.is_empty() {
			return Err!(Request(NotFound("No state found.")));
		}

		let json = serde_json::to_string_pretty(&state_at).map_err(|e| {
			err!(Database(
				"Failed to convert room state events to pretty JSON, possible invalid room \
				 state events in the database: {e:?}",
			))
		})?;

		self.write_str(&format!("```json\n{json}\n```")).await
	}
}
