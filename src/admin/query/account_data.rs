use clap::Subcommand;
use conduwuit::Result;
use conduwuit_database::Deserialized as _;
use futures::StreamExt;
use ruma::{OwnedRoomId, OwnedUserId, exports::serde::Serialize};

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
/// All the getters and iterators from src/database/key_value/account_data.rs
pub enum AccountDataCommand {
	/// Returns all changes to the account data that happened after `since`.
	ChangesSince {
		/// Full user ID
		user_id: OwnedUserId,
		/// UNIX timestamp since (u64)
		since: u64,
		/// Optional room ID of the account data
		room_id: Option<OwnedRoomId>,
	},

	/// Searches the account data for a specific kind.
	AccountDataGet {
		/// Full user ID
		user_id: OwnedUserId,
		/// Account data event type
		kind: String,
		/// Optional room ID of the account data
		room_id: Option<OwnedRoomId>,
	},
}

impl crate::Context<'_> {
	async fn changes_since(
		&self,
		user_id: OwnedUserId,
		since: u64,
		room_id: Option<OwnedRoomId>,
	) -> Result {
		let timer = tokio::time::Instant::now();
		let results: Vec<_> = self
			.services
			.account_data
			.changes_since(room_id.as_deref(), &user_id, Some(since), None)
			.collect()
			.await;
		let query_time = timer.elapsed();

		self.write_str(&format!("Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```"))
			.await
	}

	async fn account_data_get(
		&self,
		user_id: OwnedUserId,
		kind: String,
		room_id: Option<OwnedRoomId>,
	) -> Result {
		let timer = tokio::time::Instant::now();
		let result = self
			.services
			.account_data
			.get_raw(room_id.as_deref(), &user_id, &kind)
			.await;
		let query_time = timer.elapsed();

		let json = serde_json::to_string_pretty(&match room_id {
			| None => result
				.deserialized::<ruma::serde::Raw<ruma::events::AnyGlobalAccountDataEvent>>()?
				.serialize(serde_json::value::Serializer)?,
			| Some(_) => result
				.deserialized::<ruma::serde::Raw<ruma::events::AnyRoomAccountDataEvent>>()?
				.serialize(serde_json::value::Serializer)?,
		})?;

		self.write_str(&format!("Query completed in {query_time:?}:\n\n```rs\n{json}\n```"))
			.await
	}
}
