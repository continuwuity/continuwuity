use clap::Subcommand;
use conduwuit::{
	Result,
	utils::{IterStream, stream::BroadbandExt},
};
use futures::StreamExt;
use ruma::{OwnedDeviceId, OwnedUserId};

use crate::Context;

#[derive(Debug, Subcommand)]
pub enum PusherCommand {
	/// Returns all the pushers for the user.
	GetPushers {
		/// Full user ID
		user_id: OwnedUserId,
	},

	/// Deletes a specific pusher by ID
	DeletePusher {
		user_id: OwnedUserId,
		pusher_id: String,
	},

	/// Deletes all pushers for a user
	DeleteAllUser {
		user_id: OwnedUserId,
	},

	/// Deletes all pushers associated with a device ID
	DeleteAllDevice {
		user_id: OwnedUserId,
		device_id: OwnedDeviceId,
	},
}

pub(super) async fn process(subcommand: PusherCommand, context: &Context<'_>) -> Result {
	let services = context.services;

	match subcommand {
		| PusherCommand::GetPushers { user_id } => {
			let timer = tokio::time::Instant::now();
			let results = services.pusher.get_pushers(&user_id).await;
			let query_time = timer.elapsed();

			write!(context, "Query completed in {query_time:?}:\n\n```rs\n{results:#?}\n```")
		},
		| PusherCommand::DeletePusher { user_id, pusher_id } => {
			services.pusher.delete_pusher(&user_id, &pusher_id).await;
			write!(context, "Deleted pusher {pusher_id} for {user_id}.")
		},
		| PusherCommand::DeleteAllUser { user_id } => {
			let pushers = services
				.pusher
				.get_pushkeys(&user_id)
				.collect::<Vec<_>>()
				.await;
			let pusher_count = pushers.len();
			pushers
				.stream()
				.for_each(async |pushkey| {
					services.pusher.delete_pusher(&user_id, pushkey).await;
				})
				.await;
			write!(context, "Deleted {pusher_count} pushers for {user_id}.")
		},
		| PusherCommand::DeleteAllDevice { user_id, device_id } => {
			let pushers = services
				.pusher
				.get_pushkeys(&user_id)
				.map(ToOwned::to_owned)
				.broad_filter_map(async |pushkey| {
					services
						.pusher
						.get_pusher_device(&pushkey)
						.await
						.ok()
						.as_ref()
						.is_some_and(|pusher_device| pusher_device == &device_id)
						.then_some(pushkey)
				})
				.collect::<Vec<_>>()
				.await;
			let pusher_count = pushers.len();
			pushers
				.stream()
				.for_each(async |pushkey| {
					services.pusher.delete_pusher(&user_id, &pushkey).await;
				})
				.await;
			write!(context, "Deleted {pusher_count} pushers for {device_id}.")
		},
	}
	.await
}
