use clap::Subcommand;
use conduwuit::{Err, Event, Result, err, utils::stream::BroadbandExt};
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
use ruma::{OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum GraphCommand {
	/// Lists all forward extremities in a room.
	ListExtremities {
		room: OwnedRoomOrAliasId,
	},

	/// Fetches a PDU (event) by its event ID, as stored in the database. Also
	/// includes details about the PDU state.
	GetPDU {
		event_id: OwnedEventId,
	},

	/// Yields the first PDU we became aware of in a room.
	/// This is always the room's create event for local rooms, or the first
	/// membership event that introduced us to the remote room.
	#[clap(name = "first-pdu")]
	FirstPduInRoom {
		room_id: OwnedRoomId,
	},

	/// Yields the most recent PDU in a room.
	#[clap(name = "latest-pdu")]
	LatestPduInRoom {
		room_id: OwnedRoomId,
	},
}

impl crate::Context<'_> {
	pub(super) async fn list_extremities(&self, room: OwnedRoomOrAliasId) -> Result {
		let room_id = self.services.rooms.alias.resolve(&room).await?;
		let mut extremities = self
			.services
			.rooms
			.state
			.get_forward_extremities(&room_id)
			.broad_filter_map(|event_id| async move {
				self.services.rooms.timeline.get_pdu(&event_id).await.ok()
			})
			.collect::<Vec<_>>()
			.await;
		extremities.sort_by_key(|pdu| (pdu.depth, pdu.origin_server_ts));
		if extremities.is_empty() {
			return Err!("No room extremities.");
		}
		self.write_str("| Event ID | Type | Sender | Timestamp | Depth |\n")
			.await?;
		self.write_str("| -------- | ---- | ------ | --------- | ----- |\n")
			.await?;
		for pdu in extremities {
			let url = format!(
				"https://matrix.to/#/{}/{}?via={}",
				room_id,
				pdu.event_id(),
				self.services.server.name
			);
			self.write_str(&format!(
				"| [`{}`]({url}) | `{}` | `{}` | `{}` | `{}` |\n",
				pdu.event_id(),
				pdu.kind(),
				pdu.sender(),
				pdu.origin_server_ts,
				pdu.depth
			))
			.await?;
		}

		Ok(())
	}

	pub(super) async fn get_pdu(&self, event_id: OwnedEventId) -> Result {
		let mut outlier = false;
		let mut pdu_json = self
			.services
			.rooms
			.timeline
			.get_non_outlier_pdu_json(&event_id)
			.await;

		if pdu_json.is_err() {
			outlier = true;
			pdu_json = self.services.rooms.timeline.get_pdu_json(&event_id).await;
		}
		let rejected = self
			.services
			.rooms
			.pdu_metadata
			.is_event_rejected(&event_id)
			.await;
		let soft_failed = self
			.services
			.rooms
			.pdu_metadata
			.is_event_soft_failed(&event_id)
			.await;

		match pdu_json {
			| Err(_) => return Err!("PDU not found locally."),
			| Ok(json) => {
				let text = serde_json::to_string_pretty(&json)?;
				let msg = if rejected {
					"Rejected PDU:"
				} else if soft_failed {
					"Soft-failed PDU:"
				} else if outlier {
					"Outlier PDU:"
				} else {
					"Timeline PDU:"
				};
				write!(self, "{msg}\n```json\n{text}\n```")
			},
		}
		.await
	}

	pub(super) async fn first_pdu_in_room(&self, room_id: OwnedRoomId) -> Result {
		self.bail_restricted()?;

		let first_pdu = self
			.services
			.rooms
			.timeline
			.first_pdu_in_room(&room_id)
			.await
			.map(Event::into_canonical_object)
			.and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
			.map_err(|e| err!(Database("Failed to find the first PDU in database: {e:?}")))?;

		let out = format!("```json\n{first_pdu}\n```");
		self.write_str(&out).await
	}

	pub(super) async fn latest_pdu_in_room(&self, room_id: OwnedRoomId) -> Result {
		self.bail_restricted()?;

		let latest_pdu = self
			.services
			.rooms
			.timeline
			.latest_pdu_in_room(&room_id)
			.await
			.map(Event::into_canonical_object)
			.and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
			.map_err(|e| err!(Database("Failed to find the latest PDU in database: {e:?}")))?;

		let out = format!("```json\n{latest_pdu}\n```");
		self.write_str(&out).await
	}
}
