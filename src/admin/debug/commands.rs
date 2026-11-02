use std::collections::HashMap;

use conduwuit::{
	Err, Result,
	utils::{
		stream::{IterStream, ReadyExt},
		string::EMPTY,
	},
};
use futures::{StreamExt, TryStreamExt};
use lettre::message::Mailbox;
use ruma::OwnedRoomId;

use crate::PAGE_SIZE;

impl crate::Context<'_> {
	pub(super) async fn echo(&self, message: Vec<String>) -> Result {
		let message = message.join(" ");
		self.write_str(&message).await
	}

	pub(super) async fn memory_stats(&self, opts: Option<String>) -> Result {
		const OPTS: &str = "abcdefghijklmnopqrstuvwxyz";

		let opts: String = OPTS
			.chars()
			.filter(|&c| {
				let allow_any = opts.as_ref().is_some_and(|opts| opts == "*");

				let allow = allow_any || opts.as_ref().is_some_and(|opts| opts.contains(c));

				!allow
			})
			.collect();

		let stats = conduwuit::alloc::memory_stats(&opts).unwrap_or_default();

		self.write_str("```\n").await?;
		self.write_str(&stats).await?;
		self.write_str("\n```").await?;
		Ok(())
	}

	#[cfg(tokio_unstable)]

	pub(super) async fn runtime_metrics(&self) -> Result {
		let out = self.services.server.metrics.runtime_metrics().map_or_else(
			|| "Runtime metrics are not available.".to_owned(),
			|metrics| {
				format!(
					"```rs\nnum_workers: {}\nnum_alive_tasks: {}\nglobal_queue_depth: {}\n```",
					metrics.num_workers(),
					metrics.num_alive_tasks(),
					metrics.global_queue_depth()
				)
			},
		);

		self.write_str(&out).await
	}

	#[cfg(not(tokio_unstable))]
	pub(super) async fn runtime_metrics(&self) -> Result {
		self.write_str("Runtime metrics require building with `tokio_unstable`.")
			.await
	}

	#[cfg(tokio_unstable)]

	pub(super) async fn runtime_interval(&self) -> Result {
		let out = self.services.server.metrics.runtime_interval().map_or_else(
			|| "Runtime metrics are not available.".to_owned(),
			|metrics| format!("```rs\n{metrics:#?}\n```"),
		);

		self.write_str(&out).await
	}

	#[cfg(not(tokio_unstable))]
	pub(super) async fn runtime_interval(&self) -> Result {
		self.write_str("Runtime metrics require building with `tokio_unstable`.")
			.await
	}

	pub(super) async fn database_stats(
		&self,
		property: Option<String>,
		map: Option<String>,
	) -> Result {
		let map_name = map.as_ref().map_or(EMPTY, String::as_str);
		let property = property.unwrap_or_else(|| "rocksdb.stats".to_owned());
		self.services
			.db
			.iter()
			.filter(|&(&name, _)| map_name.is_empty() || map_name == name)
			.try_stream()
			.try_for_each(|(&name, map)| {
				let res = map.property(&property).expect("invalid property");
				writeln!(self, "##### {name}:\n```\n{}\n```", res.trim())
			})
			.await
	}

	pub(super) async fn database_files(&self, map: Option<String>, level: Option<i32>) -> Result {
		let mut files: Vec<_> = self.services.db.db.file_list().collect::<Result<_>>()?;

		files.sort_by_key(|f| f.name.clone());

		writeln!(self, "| lev  | sst  | keys | dels | size | column |").await?;
		writeln!(self, "| ---: | :--- | ---: | ---: | ---: | :---   |").await?;
		files
			.into_iter()
			.filter(|file| {
				map.as_deref()
					.is_none_or(|map| map == file.column_family_name)
			})
			.filter(|file| level.as_ref().is_none_or(|&level| level == file.level))
			.try_stream()
			.try_for_each(|file| {
				writeln!(
					self,
					"| {} | {:<13} | {:7}+ | {:4}- | {:9} | {} |",
					file.level,
					file.name,
					file.num_entries,
					file.num_deletions,
					file.size,
					file.column_family_name,
				)
			})
			.await
	}

	pub(super) async fn trim_memory(&self) -> Result {
		conduwuit::alloc::trim(None)?;

		writeln!(self, "done").await
	}

	pub(super) async fn send_test_email(&self) -> Result {
		self.bail_restricted()?;

		let mailer = self.services.mailer.expect_mailer()?;
		let Some(sender) = self.sender else {
			return Err!("No sender user provided in context");
		};

		let Some(email) = self
			.services
			.threepid
			.get_email_for_localpart(sender.localpart())
			.await
		else {
			return Err!("{} has no associated email address", sender);
		};

		mailer
			.send(Mailbox::new(None, email.clone()), service::mailer::messages::Test)
			.await?;

		self.write_str(&format!("Test email successfully sent to {email}"))
			.await?;

		Ok(())
	}

	pub(super) async fn rooms_by_extremity_count(&self, page: Option<usize>) -> Result {
		let page = page.unwrap_or(1);
		// My Giant Chain:tm:
		let mapped: HashMap<OwnedRoomId, u64> = self
			.services
			.rooms
			.state
			.all_forward_extremities()
			.ready_fold(HashMap::new(), move |mut map, (room_id, _)| {
				let count: u64 = map.get(&room_id).copied().unwrap_or(0);
				map.insert(room_id, count.saturating_add(1));
				map
			})
			.await
			.into_iter()
			.filter_map(|(room_id, count)| (count >= 2).then_some((room_id, count)))
			.collect();
		if mapped.is_empty() {
			return Err!("No more rooms.");
		}

		let mut rooms = mapped.keys().collect::<Vec<_>>();
		rooms.sort_by_key(|room_id| {
			mapped
				.get(*room_id)
				.copied()
				.expect("keys must have values")
		});
		rooms.reverse();

		let body = rooms
			.into_iter()
			.stream()
			.skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
			.take(PAGE_SIZE)
			.map(|room_id| {
				format!(
					"{room_id}: {}",
					mapped.get(room_id).copied().expect("keys must have values")
				)
			})
			.collect::<Vec<_>>()
			.await;

		self.write_str(&format!(
			"Rooms by extremity count ({}):\n```\n{}\n```",
			body.len(),
			body.join("\n")
		))
		.await
	}
}
