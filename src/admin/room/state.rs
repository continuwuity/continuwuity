use std::collections::HashMap;

use clap::Subcommand;
use conduwuit::{Err, Event, PduEvent, Result, debug_error, err, info, warn};
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
#[allow(unused_imports)]
// necessary to stop RustRover from importing this trait for some reason
use ruma::state_res::Event as _;
use ruma::{
	OwnedEventId, OwnedRoomOrAliasId, OwnedServerName, api::federation::event::get_room_state,
};
use service::rooms::state_compressor::HashSetCompressStateEvent;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum RoomStateCommand {
	/// Exports the room's current state as a JSON file.
	/// If an event ID is supplied, the state at that event (the state preceding
	/// it, plus any change made by the named event) is returned instead, if it
	/// is known.
	#[clap(name = "export")]
	ExportState {
		/// The room ID or alias to get the state for.
		room: OwnedRoomOrAliasId,
		/// The event ID to get the state at, if any.
		event_id: Option<OwnedEventId>,
	},

	/// Downloads the current state of a room from a remote server, and replaces
	/// the local copy of the room's state with it.
	///
	/// If you're reading this, this command does not do what you think it does.
	Transplant {
		/// The room ID or alias to replace the current state of.
		room: OwnedRoomOrAliasId,
		/// The server name to fetch the state from. Make sure this is a trusted
		/// server, as the result will be implicitly trusted.
		server_name: OwnedServerName,
		/// The latest event ID in the room to use as the "latest event".
		/// Calculated by default.
		at_event: Option<OwnedEventId>,
		/// Try to force the state even if the server thinks it isn't in the
		/// target room.
		///
		/// Using this on rooms the server has never joined may break
		/// everything.
		#[arg(long)]
		force: bool,
		// (the "may break everything" part isn't necessarily true, but it is highly likely that
		// things will actually break in an unrecoverable way, so being dramatic gets the main
		// point across)
	},
}

impl crate::Context<'_> {
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

	pub(super) async fn transplant(
		&self,
		room: OwnedRoomOrAliasId,
		server_name: OwnedServerName,
		latest_event: Option<OwnedEventId>,
		force: bool,
	) -> Result {
		self.bail_restricted()?;
		let room_id = self.services.rooms.alias.resolve(&room).await?;

		if !force
			&& !self
				.services
				.rooms
				.state_cache
				.server_in_room(&self.services.server.name, &room_id)
				.await
		{
			return Err!(
				"We are not participating in the room / we don't know about the room ID."
			);
		}

		let at_event_id = match latest_event {
			| Some(event_id) => event_id,
			| None => self
				.services
				.rooms
				.timeline
				.latest_pdu_in_room(&room_id)
				.await
				.map_err(|_| err!(Database("Failed to find the latest PDU in database")))?
				.event_id()
				.to_owned(),
		};

		let room_version = self.services.rooms.state.get_room_version(&room_id).await?;
		let room_version_rules = room_version.rules().unwrap();

		let mut state: HashMap<u64, OwnedEventId> = HashMap::new();

		let state_lock = self.services.rooms.state.mutex.lock(&*room_id).await;

		// TODO: Use /state_ids and fetch events we're missing atomically,
		// instead of /state, which is very large and frequently times out.
		let remote_state_response = self
			.services
			.sending
			.send_federation_request(
				&server_name,
				get_room_state::v1::Request::new(at_event_id.clone(), room_id.clone()),
			)
			.await?;

		for pdu in remote_state_response.pdus.clone() {
			match self
				.services
				.rooms
				.event_handler
				.parse_incoming_pdu(&pdu, Some(&room_version_rules))
				.await
			{
				| Ok(t) => t,
				| Err(e) => {
					warn!("Could not parse PDU, ignoring: {e}");
					continue;
				},
			};
		}

		info!("Going through room_state response PDUs");
		for result in remote_state_response.pdus.iter().map(|pdu| {
			self.services
				.server_keys
				.verify_event_json_no_fetch_add_event_id(pdu, &room_version_rules)
		}) {
			let Ok((event_id, value)) = result.await else {
				continue;
			};

			let pdu = PduEvent::from_id_val(&event_id, value.clone()).map_err(|e| {
				debug_error!(
					"Invalid PDU in fetching remote room state PDUs response: {value:#?}"
				);
				err!(BadServerResponse(debug_error!("Invalid PDU in send_join response: {e:?}")))
			})?;

			self.services
				.rooms
				.outlier
				.add_pdu_outlier(&event_id, &value);

			if let Some(state_key) = &pdu.state_key {
				let shortstatekey = self
					.services
					.rooms
					.short
					.get_or_create_shortstatekey(&pdu.kind.to_string().into(), state_key)
					.await;

				state.insert(shortstatekey, pdu.event_id.clone());
				self.services
					.rooms
					.pdu_metadata
					.clear_pdu_markers(pdu.event_id());
			}
		}

		info!("Going through auth_chain response");
		for result in remote_state_response.auth_chain.iter().map(|pdu| {
			self.services
				.server_keys
				.verify_event_json_no_fetch_add_event_id(pdu, &room_version_rules)
		}) {
			let Ok((event_id, value)) = result.await else {
				continue;
			};

			self.services
				.rooms
				.outlier
				.add_pdu_outlier(&event_id, &value);
			self.services
				.rooms
				.pdu_metadata
				.clear_pdu_markers(&event_id);
		}

		info!("Resolving new room state");
		let new_room_state = self
			.services
			.rooms
			.event_handler
			.resolve_state(&room_id, &room_version_rules, state)
			.await?;

		info!("Compressing new room state");
		let HashSetCompressStateEvent {
			shortstatehash: short_state_hash,
			added,
			removed,
		} = self
			.services
			.rooms
			.state_compressor
			.save_state(room_id.clone().as_ref(), new_room_state)
			.await?;

		info!("Forcing new room state");
		self.services
			.rooms
			.state
			.force_state(room_id.clone().as_ref(), short_state_hash, added, removed, &state_lock)
			.await?;

		info!("Replacing forward extremities");
		self.services
			.rooms
			.state
			.replace_forward_extremities(
				&room_id,
				std::iter::once(at_event_id.as_ref()),
				&state_lock,
			)
			.await;

		info!("Updating joined counts for room");
		self.services
			.rooms
			.state_cache
			.update_joined_count(&room_id)
			.await;

		self.write_str("Successfully forced the room state from the requested remote server.")
			.await
	}
}
