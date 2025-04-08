use clap::Subcommand;
use conduwuit::Result;
use ruma::OwnedUserId;

use crate::Command;

#[derive(Debug, Subcommand)]
pub(crate) enum PusherCommand {
	/// - Returns all the pushers for the user.
	GetPushers {
		/// Full user ID
		user_id: OwnedUserId,
	},
}

pub(super) async fn process(subcommand: PusherCommand, context: &Command<'_>) -> Result {
	let services = context.services;

	match subcommand {
		| PusherCommand::GetPushers { user_id } => {
			let timer = tokio::time::Instant::now();
			let results = services.pusher.get_pushers(&user_id).await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
	}
	.await
}
