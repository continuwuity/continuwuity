use clap::Subcommand;
use conduwuit::Result;
use futures::StreamExt;
use ruma::{OwnedRoomAliasId, OwnedRoomId};

use crate::Context;

#[derive(Debug, Subcommand)]
/// All the getters and iterators from src/database/key_value/rooms/alias.rs
pub(crate) enum RoomAliasCommand {
	ResolveLocalAlias {
		/// Full room alias
		alias: OwnedRoomAliasId,
	},

	/// - Iterator of all our local room aliases for the room ID
	LocalAliasesForRoom {
		/// Full room ID
		room_id: OwnedRoomId,
	},

	/// - Iterator of all our local aliases in our database with their room IDs
	AllLocalAliases,
}

/// All the getters and iterators in src/database/key_value/rooms/alias.rs
pub(super) async fn process(subcommand: RoomAliasCommand, context: &Context<'_>) -> Result {
	let services = context.services;

	match subcommand {
		| RoomAliasCommand::ResolveLocalAlias { alias } => {
			let timer = tokio::time::Instant::now();
			let results = services.rooms.alias.resolve_local_alias(&alias).await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
		| RoomAliasCommand::LocalAliasesForRoom { room_id } => {
			let timer = tokio::time::Instant::now();
			let aliases: Vec<_> = services
				.rooms
				.alias
				.local_aliases_for_room(&room_id)
				.map(ToOwned::to_owned)
				.collect()
				.await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{aliases:#?}\n```")
		},
		| RoomAliasCommand::AllLocalAliases => {
			let timer = tokio::time::Instant::now();
			let aliases = services
				.rooms
				.alias
				.all_local_aliases()
				.map(|(room_id, alias)| (room_id.to_owned(), alias.to_owned()))
				.collect::<Vec<_>>()
				.await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{aliases:#?}\n```")
		},
	}
	.await
}
