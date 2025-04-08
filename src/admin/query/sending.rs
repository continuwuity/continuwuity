use clap::Subcommand;
use conduwuit::Result;
use futures::StreamExt;
use ruma::{OwnedServerName, OwnedUserId, events::room::message::RoomMessageEventContent};
use service::sending::Destination;

use crate::Command;

#[derive(Debug, Subcommand)]
/// All the getters and iterators from src/database/key_value/sending.rs
pub(crate) enum SendingCommand {
	/// - Queries database for all `servercurrentevent_data`
	ActiveRequests,

	/// - Queries database for `servercurrentevent_data` but for a specific
	///   destination
	///
	/// This command takes only *one* format of these arguments:
	///
	/// appservice_id
	/// server_name
	/// user_id AND push_key
	///
	/// See src/service/sending/mod.rs for the definition of the `Destination`
	/// enum
	ActiveRequestsFor {
		#[arg(short, long)]
		appservice_id: Option<String>,
		#[arg(short, long)]
		server_name: Option<OwnedServerName>,
		#[arg(short, long)]
		user_id: Option<OwnedUserId>,
		#[arg(short, long)]
		push_key: Option<String>,
	},

	/// - Queries database for `servernameevent_data` which are the queued up
	///   requests that will eventually be sent
	///
	/// This command takes only *one* format of these arguments:
	///
	/// appservice_id
	/// server_name
	/// user_id AND push_key
	///
	/// See src/service/sending/mod.rs for the definition of the `Destination`
	/// enum
	QueuedRequests {
		#[arg(short, long)]
		appservice_id: Option<String>,
		#[arg(short, long)]
		server_name: Option<OwnedServerName>,
		#[arg(short, long)]
		user_id: Option<OwnedUserId>,
		#[arg(short, long)]
		push_key: Option<String>,
	},

	GetLatestEduCount {
		server_name: OwnedServerName,
	},
}

/// All the getters and iterators in key_value/sending.rs
pub(super) async fn process(subcommand: SendingCommand, context: &Command<'_>) -> Result {
	let c = reprocess(subcommand, context).await?;
	context.write_str(c.body()).await?;
	Ok(())
}

/// All the getters and iterators in key_value/sending.rs
pub(super) async fn reprocess(
	subcommand: SendingCommand,
	context: &Command<'_>,
) -> Result<RoomMessageEventContent> {
	let services = context.services;

	match subcommand {
		| SendingCommand::ActiveRequests => {
			let timer = tokio::time::Instant::now();
			let results = services.sending.db.active_requests();
			let active_requests = results.collect::<Vec<_>>().await;
			let query_time = timer.elapsed();

			Ok(RoomMessageEventContent::notice_markdown(format!(
				"Query completed in {query_time:?}:\n\n```rs\n{active_requests:#?}\n```"
			)))
		},
		| SendingCommand::QueuedRequests {
			appservice_id,
			server_name,
			user_id,
			push_key,
		} => {
			if appservice_id.is_none()
				&& server_name.is_none()
				&& user_id.is_none()
				&& push_key.is_none()
			{
				return Ok(RoomMessageEventContent::text_plain(
					"An appservice ID, server name, or a user ID with push key must be \
					 specified via arguments. See --help for more details.",
				));
			}
			let timer = tokio::time::Instant::now();
			let results = match (appservice_id, server_name, user_id, push_key) {
				| (Some(appservice_id), None, None, None) => {
					if appservice_id.is_empty() {
						return Ok(RoomMessageEventContent::text_plain(
							"An appservice ID, server name, or a user ID with push key must be \
							 specified via arguments. See --help for more details.",
						));
					}

					services
						.sending
						.db
						.queued_requests(&Destination::Appservice(appservice_id))
				},
				| (None, Some(server_name), None, None) => services
					.sending
					.db
					.queued_requests(&Destination::Federation(server_name)),
				| (None, None, Some(user_id), Some(push_key)) => {
					if push_key.is_empty() {
						return Ok(RoomMessageEventContent::text_plain(
							"An appservice ID, server name, or a user ID with push key must be \
							 specified via arguments. See --help for more details.",
						));
					}

					services
						.sending
						.db
						.queued_requests(&Destination::Push(user_id, push_key))
				},
				| (Some(_), Some(_), Some(_), Some(_)) => {
					return Ok(RoomMessageEventContent::text_plain(
						"An appservice ID, server name, or a user ID with push key must be \
						 specified via arguments. Not all of them See --help for more details.",
					));
				},
				| _ => {
					return Ok(RoomMessageEventContent::text_plain(
						"An appservice ID, server name, or a user ID with push key must be \
						 specified via arguments. See --help for more details.",
					));
				},
			};

			let queued_requests = results.collect::<Vec<_>>().await;
			let query_time = timer.elapsed();

			Ok(RoomMessageEventContent::notice_markdown(format!(
				"Query completed in {query_time:?}:\n\n```rs\n{queued_requests:#?}\n```"
			)))
		},
		| SendingCommand::ActiveRequestsFor {
			appservice_id,
			server_name,
			user_id,
			push_key,
		} => {
			if appservice_id.is_none()
				&& server_name.is_none()
				&& user_id.is_none()
				&& push_key.is_none()
			{
				return Ok(RoomMessageEventContent::text_plain(
					"An appservice ID, server name, or a user ID with push key must be \
					 specified via arguments. See --help for more details.",
				));
			}

			let timer = tokio::time::Instant::now();
			let results = match (appservice_id, server_name, user_id, push_key) {
				| (Some(appservice_id), None, None, None) => {
					if appservice_id.is_empty() {
						return Ok(RoomMessageEventContent::text_plain(
							"An appservice ID, server name, or a user ID with push key must be \
							 specified via arguments. See --help for more details.",
						));
					}

					services
						.sending
						.db
						.active_requests_for(&Destination::Appservice(appservice_id))
				},
				| (None, Some(server_name), None, None) => services
					.sending
					.db
					.active_requests_for(&Destination::Federation(server_name)),
				| (None, None, Some(user_id), Some(push_key)) => {
					if push_key.is_empty() {
						return Ok(RoomMessageEventContent::text_plain(
							"An appservice ID, server name, or a user ID with push key must be \
							 specified via arguments. See --help for more details.",
						));
					}

					services
						.sending
						.db
						.active_requests_for(&Destination::Push(user_id, push_key))
				},
				| (Some(_), Some(_), Some(_), Some(_)) => {
					return Ok(RoomMessageEventContent::text_plain(
						"An appservice ID, server name, or a user ID with push key must be \
						 specified via arguments. Not all of them See --help for more details.",
					));
				},
				| _ => {
					return Ok(RoomMessageEventContent::text_plain(
						"An appservice ID, server name, or a user ID with push key must be \
						 specified via arguments. See --help for more details.",
					));
				},
			};

			let active_requests = results.collect::<Vec<_>>().await;
			let query_time = timer.elapsed();

			Ok(RoomMessageEventContent::notice_markdown(format!(
				"Query completed in {query_time:?}:\n\n```rs\n{active_requests:#?}\n```"
			)))
		},
		| SendingCommand::GetLatestEduCount { server_name } => {
			let timer = tokio::time::Instant::now();
			let results = services.sending.db.get_latest_educount(&server_name).await;
			let query_time = timer.elapsed();

			Ok(RoomMessageEventContent::notice_markdown(format!(
				"Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```"
			)))
		},
	}
}
