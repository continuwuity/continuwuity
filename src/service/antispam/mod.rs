use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use conduwuit::{Result, Server, config::Antispam, debug, metrics::KeyValue};
use ruma::{
	OwnedRoomId, OwnedUserId,
	api::{auth_scheme::AppserviceToken, path_builder::VersionHistory},
};
use ruminuwuity::{draupnir_antispam, meowlnir_antispam};

use crate::{client, config, sending, service::Dep};

struct Services {
	server: Arc<Server>,
	config: Dep<config::Service>,
	client: Dep<client::Service>,
}

pub struct Service {
	services: Services,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				client: args.depend::<client::Service>("client"),
				config: args.depend::<config::Service>("config"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	async fn send_antispam_request<T>(
		&self,
		base_url: &str,
		secret: &str,
		request: T,
	) -> Result<T::IncomingResponse>
	where
		T: ruma::api::OutgoingRequest<
				Authentication = AppserviceToken,
				PathBuilder = VersionHistory,
			> + Debug
			+ Send,
	{
		sending::antispam::send_antispam_request(
			&self.services.client.appservice,
			base_url,
			secret,
			request,
		)
		.await
	}

	/// Checks with the antispam service whether `inviter` may invite `invitee`
	/// to `room_id`.
	///
	/// If no antispam service is configured, this always returns `Ok(())`.
	/// If an error is returned, the invite should be blocked - the antispam
	/// service was unreachable, or refused the invite.
	pub async fn user_may_invite(
		&self,
		inviter: OwnedUserId,
		invitee: OwnedUserId,
		room_id: OwnedRoomId,
	) -> Result<()> {
		if let Some(config) = &self.services.config.antispam {
			let result = if let Some(meowlnir) = &config.meowlnir {
				debug!(?room_id, ?inviter, ?invitee, "Asking meowlnir for user_may_invite");
				self.send_antispam_request(
					meowlnir.base_url.as_str(),
					&meowlnir.secret,
					meowlnir_antispam::user_may_invite::v1::Request::new(
						meowlnir.management_room.clone(),
						inviter,
						invitee,
						room_id,
					),
				)
				.await
				.inspect(|_| debug!("meowlnir allowed the invite"))
				.inspect_err(|e| debug!("meowlnir denied the invite: {e:?}"))
				.map(|_| ())
			} else if let Some(draupnir) = &config.draupnir {
				debug!(?room_id, ?inviter, ?invitee, "Asking draupnir for user_may_invite");
				self.send_antispam_request(
					draupnir.base_url.as_str(),
					&draupnir.secret,
					draupnir_antispam::user_may_invite::v1::Request::new(
						room_id, inviter, invitee,
					),
				)
				.await
				.inspect(|_| debug!("draupnir allowed the invite"))
				.inspect_err(|e| debug!("draupnir denied the invite: {e:?}"))
				.map(|_| ())
			} else {
				Ok(())
			};
			self.record("user_may_invite", &result);
			return result;
		}
		Ok(())
	}

	/// Checks with the antispam service whether `user_id` may join `room_id`.
	pub async fn user_may_join_room(
		&self,
		user_id: OwnedUserId,
		room_id: OwnedRoomId,
		is_invited: bool,
	) -> Result<()> {
		if let Some(config) = &self.services.config.antispam {
			let result = if let Some(meowlnir) = &config.meowlnir {
				debug!(?room_id, ?user_id, ?is_invited, "Asking meowlnir for user_may_join_room");
				self.send_antispam_request(
					meowlnir.base_url.as_str(),
					&meowlnir.secret,
					meowlnir_antispam::user_may_join_room::v1::Request::new(
						meowlnir.management_room.clone(),
						user_id,
						room_id,
						is_invited,
					),
				)
				.await
				.inspect(|_| debug!("meowlnir allowed the join"))
				.inspect_err(|e| debug!("meowlnir denied the join: {e:?}"))
				.map(|_| ())
			} else if let Some(draupnir) = &config.draupnir {
				debug!(?room_id, ?user_id, ?is_invited, "Asking draupnir for user_may_join_room");
				self.send_antispam_request(
					draupnir.base_url.as_str(),
					&draupnir.secret,
					draupnir_antispam::user_may_join_room::v1::Request::new(
						user_id, room_id, is_invited,
					),
				)
				.await
				.inspect(|_| debug!("draupnir allowed the join"))
				.inspect_err(|e| debug!("draupnir denied the join: {e:?}"))
				.map(|_| ())
			} else {
				Ok(())
			};
			self.record("user_may_join_room", &result);
			return result;
		}
		Ok(())
	}

	/// Checks with Meowlnir whether the incoming federated `make_join` request
	/// should be allowed. Applies the `fi.mau.spam_checker` join rule.
	pub async fn meowlnir_accept_make_join(
		&self,
		room_id: OwnedRoomId,
		user_id: OwnedUserId,
	) -> Result<()> {
		if let Some(Antispam { meowlnir: Some(meowlnir), .. }) = &self.services.config.antispam {
			debug!(?room_id, ?user_id, "Asking meowlnir for accept_make_join");
			let result = self
				.send_antispam_request(
					meowlnir.base_url.as_str(),
					&meowlnir.secret,
					meowlnir_antispam::accept_make_join::v1::Request::new(
						meowlnir.management_room.clone(),
						user_id,
						room_id,
					),
				)
				.await
				.inspect(|_| debug!("meowlnir allowed the make_join"))
				.inspect_err(|e| debug!("meowlnir denied the make_join: {e:?}"))
				.map(|_| ());
			self.record("accept_make_join", &result);
			result
		} else {
			Ok(())
		}
	}

	fn record(&self, check: &'static str, result: &Result) {
		let outcome = if result.is_ok() { "allowed" } else { "denied" };
		self.services
			.server
			.metrics
			.instruments
			.antispam_checks
			.add(1, &[KeyValue::new("check", check), KeyValue::new("outcome", outcome)]);
	}

	/// Returns whether all joins should be checked with Meowlnir.
	/// Is always false if Meowlnir is not configured.
	pub fn check_all_joins(&self) -> bool {
		if let Some(Antispam { meowlnir: Some(cfg), .. }) = &self.services.config.antispam {
			cfg.check_all_joins
		} else {
			false
		}
	}
}
