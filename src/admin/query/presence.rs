use clap::Subcommand;
use conduwuit::Result;
use futures::StreamExt;
use ruma::OwnedUserId;

use crate::Context;

#[derive(Debug, Subcommand)]
/// All the getters and iterators from src/database/key_value/presence.rs
pub enum PresenceCommand {
	/// - Returns the latest presence event for the given user.
	GetPresence {
		/// Full user ID
		user_id: OwnedUserId,
	},

	/// - Iterator of the most recent presence updates that happened after the
	///   event with id `since`.
	PresenceSince {
		/// UNIX timestamp since (u64)
		since: u64,
	},
}

/// All the getters and iterators in key_value/presence.rs
pub(super) async fn process(subcommand: PresenceCommand, context: &Context<'_>) -> Result {
	let services = context.services;

	match subcommand {
		| PresenceCommand::GetPresence { user_id } => {
			let timer = tokio::time::Instant::now();
			let results = services.presence.get_presence(&user_id).await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
		| PresenceCommand::PresenceSince { since } => {
			let timer = tokio::time::Instant::now();
			let results: Vec<(_, _, _)> = services
				.presence
				.presence_since(since)
				.map(|(user_id, count, bytes)| (user_id.to_owned(), count, bytes.to_vec()))
				.collect()
				.await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
	}
	.await
}
