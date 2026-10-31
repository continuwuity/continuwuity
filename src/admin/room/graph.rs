use clap::Subcommand;
use conduwuit::{Err, Event, Result, utils::stream::BroadbandExt};
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
use ruma::OwnedRoomOrAliasId;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum GraphCommand {
	/// Lists all forward extremities in a room.
	ListExtremities {
		room: OwnedRoomOrAliasId,
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
}
