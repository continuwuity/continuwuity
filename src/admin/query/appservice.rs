use clap::Subcommand;
use conduwuit::Result;
use futures::TryStreamExt;

use crate::Context;

#[derive(Debug, Subcommand)]
/// All the getters and iterators from src/database/key_value/appservice.rs
pub enum AppserviceCommand {
	/// - Gets the appservice registration info/details from the ID as a string
	GetRegistration {
		/// Appservice registration ID
		appservice_id: String,
	},

	/// - Gets all appservice registrations with their ID and registration info
	All,
}

/// All the getters and iterators from src/database/key_value/appservice.rs
pub(super) async fn process(subcommand: AppserviceCommand, context: &Context<'_>) -> Result {
	let services = context.services;

	match subcommand {
		| AppserviceCommand::GetRegistration { appservice_id } => {
			let timer = tokio::time::Instant::now();
			let results = services.appservice.get_registration(&appservice_id).await;

			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
		| AppserviceCommand::All => {
			let timer = tokio::time::Instant::now();
			let results: Vec<_> = services.appservice.iter_db_ids().try_collect().await?;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
	}
	.await
}
