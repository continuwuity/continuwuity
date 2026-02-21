//! # Announcements service
//!
//! This service is responsible for checking for announcements and sending them
//! to the client.
//!
//! It is used to send announcements to the admin room and logs.
//! Annuncements are stored in /docs/static/announcements right now.
//! The highest seen announcement id is stored in the database. When the
//! announcement check is run, all announcements with an ID higher than those
//! seen before are printed to the console and sent to the admin room.
//!
//! Old announcements should be deleted to avoid spamming the room on first
//! install.
//!
//! Announcements are displayed as markdown in the admin room, but plain text in
//! the console.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{Result, Server, debug, error, warn};
use database::{Deserialized, Map};
use ruma::events::{Mentions, room::message::RoomMessageEventContent};
use serde::Deserialize;
use tokio::{
	sync::Notify,
	time::{MissedTickBehavior, interval},
};

use crate::{Dep, admin, client, globals};

pub struct Service {
	interval: Duration,
	interrupt: Notify,
	db: Arc<Map>,
	services: Services,
}

struct Services {
	admin: Dep<admin::Service>,
	client: Dep<client::Service>,
	globals: Dep<globals::Service>,
	server: Arc<Server>,
}

#[derive(Debug, Deserialize)]
struct CheckForAnnouncementsResponse {
	announcements: Vec<CheckForAnnouncementsResponseEntry>,
}

#[derive(Debug, Deserialize)]
struct CheckForAnnouncementsResponseEntry {
	id: u64,
	date: Option<String>,
	message: String,
	#[serde(default, skip_serializing_if = "bool::not")]
	mention_room: bool,
}

const CHECK_FOR_ANNOUNCEMENTS_URL: &str =
	"https://continuwuity.org/.well-known/continuwuity/announcements";
const CHECK_FOR_ANNOUNCEMENTS_INTERVAL: u64 = 7200; // 2 hours
const LAST_CHECK_FOR_ANNOUNCEMENTS_ID: &[u8; 25] = b"last_seen_announcement_id";
// In conduwuit, this was under b"a"

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			interval: Duration::from_secs(CHECK_FOR_ANNOUNCEMENTS_INTERVAL),
			interrupt: Notify::new(),
			db: args.db["global"].clone(),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				admin: args.depend::<admin::Service>("admin"),
				client: args.depend::<client::Service>("client"),
				server: args.server.clone(),
			},
		}))
	}

	#[tracing::instrument(skip_all, name = "announcements", level = "debug")]
	async fn worker(self: Arc<Self>) -> Result<()> {
		if !self.services.globals.allow_announcements_check() {
			debug!("Disabling announcements check");
			return Ok(());
		}

		// Run the first check immediately and send errors to admin room
		if let Err(e) = self.check().await {
			error!(?e, "Failed to check for announcements on startup");
			self.services
				.admin
				.send_message(RoomMessageEventContent::text_plain(format!(
					"Failed to check for announcements on startup: {e}"
				)))
				.await
				.ok();
		}

		let first_check_jitter = {
			let jitter_percent = rand::random_range(-50.0..=10.0);
			self.interval.mul_f64(1.0 + jitter_percent / 100.0)
		};

		let mut i = interval(self.interval);
		i.set_missed_tick_behavior(MissedTickBehavior::Delay);
		i.reset_after(first_check_jitter);
		loop {
			tokio::select! {
				() = self.interrupt.notified() => break,
				_ = i.tick() => (),
			}

			if let Err(e) = self.check().await {
				warn!(?e, "Failed to check for announcements");
			}
		}

		Ok(())
	}

	fn interrupt(&self) { self.interrupt.notify_waiters(); }

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	#[tracing::instrument(skip_all)]
	async fn check(&self) -> Result<()> {
		debug_assert!(self.services.server.running(), "server must not be shutting down");

		let response = self
			.services
			.client
			.default
			.get(CHECK_FOR_ANNOUNCEMENTS_URL)
			.send()
			.await?
			.text()
			.await?;

		let response = serde_json::from_str::<CheckForAnnouncementsResponse>(&response)?;
		for announcement in &response.announcements {
			if announcement.id > self.last_check_for_announcements_id().await {
				self.handle(announcement).await;
				self.update_check_for_announcements_id(announcement.id);
			}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn handle(&self, announcement: &CheckForAnnouncementsResponseEntry) {
		let mut message = RoomMessageEventContent::text_markdown(format!(
			"### New announcement{}\n\n{}",
			announcement
				.date
				.as_ref()
				.map_or_else(String::new, |date| format!(" - `{date}`")),
			announcement.message
		));

		if announcement.mention_room {
			message = message.add_mentions(Mentions::with_room_mention());
		}

		self.services.admin.send_message(message).await.ok();
	}

	#[inline]
	pub fn update_check_for_announcements_id(&self, id: u64) {
		self.db.raw_put(LAST_CHECK_FOR_ANNOUNCEMENTS_ID, id);
	}

	pub async fn last_check_for_announcements_id(&self) -> u64 {
		self.db
			.get(LAST_CHECK_FOR_ANNOUNCEMENTS_ID)
			.await
			.deserialized()
			.unwrap_or(0_u64)
	}
}
