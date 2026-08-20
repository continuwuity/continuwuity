mod helpers;
mod vapid;

use std::time::Duration;

use conduwuit_core::{Err, Result, debug, debug_warn, err, utils, warn};
use conduwuit_database::{Deserialized, Json};
use futures::StreamExt;
pub(super) use helpers::{decode_auth, decode_pushkey};
use http::StatusCode;
use ruma::{
	UserId,
	api::push_gateway::send_event_notification::v1::{Notification, NotificationPriority},
};
use ruminuwuity::pushers::{Pusher, WebPushPusherData};
use serde::{Deserialize, Serialize};
use serde_json::json;
pub(super) use vapid::Vapid;
use web_push_native::WebPushBuilder;

use self::helpers::{build_payload, tokens_match};
use super::Service;

const TTL: Duration = Duration::from_hours(48);

const ACK_TOKEN_TTL_MS: u64 = 5 * 60 * 1000;
const ACK_TOKEN_LENGTH: usize = 32;

pub(super) const MAX_DEFAULT_PAYLOAD: usize = 1024;

const BACKOFF_BASE: Duration = Duration::from_mins(1);
const BACKOFF_MAX: Duration = Duration::from_hours(24);

#[derive(Clone, Copy, Debug)]
pub enum ActivationOutcome {
	Activated,
	NoSuchPusher,
	UnknownToken,
	Expired,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct WebPushState {
	pub(super) activated: bool,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(super) ack_token: Option<String>,

	#[serde(default)]
	pub(super) token_expires_at: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct WebPushBackoff {
	#[serde(default)]
	next_attempt_at: u64,

	#[serde(default)]
	failure_count: u32,
}

impl Service {
	/// Returns the VAPID public key when Web Push is configured.
	pub fn webpush_vapid_public_key(&self) -> Option<&str> {
		self.vapid.as_ref().map(Vapid::public_key)
	}

	/// Reports whether a Web Push pusher has completed its activation
	/// handshake.
	pub async fn is_webpush_activated(&self, sender: &UserId, pushkey: &str) -> bool {
		self.webpush_state(sender, pushkey).await.activated
	}

	pub(super) async fn webpush_state(&self, sender: &UserId, pushkey: &str) -> WebPushState {
		self.db
			.senderkey_webpushstate
			.qry(&(sender, pushkey))
			.await
			.deserialized()
			.unwrap_or_default()
	}

	fn put_webpush_state(&self, sender: &UserId, pushkey: &str, state: &WebPushState) {
		self.db
			.senderkey_webpushstate
			.put((sender, pushkey), Json(state));
	}

	pub(super) async fn webpush_needs_activation(
		&self,
		sender: &UserId,
		pusher: &Pusher,
		previous: Option<&Pusher>,
	) -> bool {
		let Some(data) = pusher.kind.as_webpush() else {
			return false;
		};

		let Some(old) = previous.and_then(|previous| previous.kind.as_webpush()) else {
			return true;
		};

		old.url != data.url
			|| old.auth != data.auth
			|| !self
				.webpush_state(sender, pusher.ids.pushkey.as_str())
				.await
				.activated
	}

	pub(super) fn begin_webpush_activation(&self, sender: &UserId, pusher: &Pusher) {
		// Send a one-time encrypted token before allowing notification delivery.
		let token = utils::rand::string(ACK_TOKEN_LENGTH);
		let state = WebPushState {
			activated: false,
			ack_token: Some(token.clone()),
			token_expires_at: utils::millis_since_unix_epoch().saturating_add(ACK_TOKEN_TTL_MS),
		};

		self.put_webpush_state(sender, pusher.ids.pushkey.as_str(), &state);
		self.db
			.senderkey_webpushbackoff
			.del((sender, pusher.ids.pushkey.as_str()));

		let Some(service) = self.me.upgrade() else {
			return;
		};
		let sender = sender.to_owned();
		let pusher = pusher.clone();
		let payload = json!({ "app_id": pusher.ids.app_id, "ack_token": token });

		self.services.server.runtime().spawn(async move {
			let Some(data) = pusher.kind.as_webpush() else {
				return;
			};

			if let Err(e) = service
				.deliver_webpush(&sender, &pusher, data, payload.to_string().into_bytes(), true)
				.await
			{
				debug_warn!(
					%sender, app_id = %pusher.ids.app_id,
					"Failed to send web push validation token: {e}"
				);
			}
		});
	}

	#[tracing::instrument(skip(self, ack_token), level = "debug")]
	/// Activates the pusher whose pending token matches the client
	/// acknowledgement.
	pub async fn activate_webpush_pusher(
		&self,
		sender: &UserId,
		app_id: &str,
		ack_token: &str,
	) -> Result<ActivationOutcome> {
		let candidates: Vec<String> = self
			.get_pushkeys(sender)
			.map(ToOwned::to_owned)
			.filter_map(async |pushkey| {
				let pusher = self.get_pusher(sender, &pushkey).await.ok()?;

				(pusher.kind.as_webpush().is_some() && pusher.ids.app_id == app_id)
					.then_some(pushkey)
			})
			.collect()
			.await;

		if candidates.is_empty() {
			return Ok(ActivationOutcome::NoSuchPusher);
		}

		for pushkey in candidates {
			let mut state = self.webpush_state(sender, &pushkey).await;
			if !state
				.ack_token
				.as_deref()
				.is_some_and(|token| tokens_match(token, ack_token))
			{
				continue;
			}

			state.ack_token = None;
			if state.token_expires_at < utils::millis_since_unix_epoch() {
				self.put_webpush_state(sender, &pushkey, &state);
				return Ok(ActivationOutcome::Expired);
			}

			state.activated = true;
			self.put_webpush_state(sender, &pushkey, &state);
			self.db.senderkey_webpushbackoff.del((sender, &pushkey));

			return Ok(ActivationOutcome::Activated);
		}

		Ok(ActivationOutcome::UnknownToken)
	}

	#[tracing::instrument(skip(self, pusher, data, notify), level = "debug")]
	/// Delivers a notification only after the pusher has completed activation.
	pub(super) async fn send_webpush_notice(
		&self,
		user: &UserId,
		pusher: &Pusher,
		data: &WebPushPusherData,
		notify: Notification,
	) -> Result {
		let pushkey = pusher.ids.pushkey.as_str();
		if !self.webpush_state(user, pushkey).await.activated {
			debug!(%user, %pushkey, "Web push pusher is not activated yet, skipping");
			return Ok(());
		}

		let backoff: WebPushBackoff = self
			.db
			.senderkey_webpushbackoff
			.qry(&(user, pushkey))
			.await
			.deserialized()
			.unwrap_or_default();

		if utils::millis_since_unix_epoch() < backoff.next_attempt_at {
			debug!(%user, %pushkey, "Web push pusher is backing off, dropping notification");
			return Ok(());
		}

		let urgent = notify.prio == NotificationPriority::High;
		let payload = build_payload(&notify, &data.data)?;

		self.deliver_webpush(user, pusher, data, payload, urgent)
			.await
	}

	#[tracing::instrument(skip(self, pusher, data, payload), level = "debug")]
	/// Encrypts and sends a Web Push request, deleting invalid subscriptions
	/// and backing off transient failures.
	async fn deliver_webpush(
		&self,
		user: &UserId,
		pusher: &Pusher,
		data: &WebPushPusherData,
		payload: Vec<u8>,
		urgent: bool,
	) -> Result {
		let Some(vapid) = self.vapid.as_ref() else {
			return Err!("Web push is not enabled");
		};

		let contact = self
			.services
			.server
			.config
			.well_known
			.support_page
			.as_ref()
			.map(ToString::to_string)
			.ok_or_else(|| {
				err!(Config(
					"well_known.support_page",
					"No support page configured for Web Push VAPID contact"
				))
			})?;

		let endpoint = self.validate_push_url(&data.url, true)?;
		let authorization = vapid.authorization(&endpoint, &contact)?;

		let mut request = WebPushBuilder::new(
			data.url
				.parse()
				.map_err(|e| err!(Request(InvalidParam("Pusher URL is not a valid URI: {e}"))))?,
			decode_pushkey(pusher.ids.pushkey.as_str())?,
			decode_auth(&data.auth)?,
		)
		.with_valid_duration(TTL)
		.build(payload)
		.map_err(|e| err!("Failed to encrypt web push message: {e}"))?;

		let headers = request.headers_mut();
		headers.insert(http::header::AUTHORIZATION, authorization.parse()?);
		headers.insert("urgency", if urgent { "high" } else { "normal" }.parse()?);

		let pushkey = pusher.ids.pushkey.as_str();
		let (status, _version, headers, _body) = self
			.execute_pusher_request(
				&self.services.client.webpush,
				reqwest::Request::try_from(request)?,
			)
			.await?;

		if status.is_success() {
			self.db.senderkey_webpushbackoff.del((user, pushkey));

			return Ok(());
		}

		if matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::GONE) {
			warn!(
				%user, %pushkey,
				"Push service rejected the subscription ({status}), deleting pusher"
			);
			self.delete_pusher(user, pushkey).await;

			return Ok(());
		}

		if status.is_redirection() {
			warn!(%user, %pushkey, "Push service redirected ({status}); not following it");
		}

		let retry_after = (status == StatusCode::TOO_MANY_REQUESTS)
			.then(|| {
				headers
					.get(http::header::RETRY_AFTER)
					.and_then(|value| value.to_str().ok())
					.and_then(|value| value.parse::<u64>().ok())
			})
			.flatten();

		self.back_off_webpush(user, pushkey, retry_after).await;

		Err!(BadServerResponse(warn!(
			%user, %pushkey,
			"Push service returned unsuccessful HTTP response: {status}"
		)))
	}

	async fn back_off_webpush(&self, user: &UserId, pushkey: &str, retry_after: Option<u64>) {
		let key = (user, pushkey);
		let mut backoff: WebPushBackoff = self
			.db
			.senderkey_webpushbackoff
			.qry(&key)
			.await
			.deserialized()
			.unwrap_or_default();

		backoff.failure_count = backoff.failure_count.saturating_add(1);

		let delay = retry_after.map_or_else(
			|| {
				BACKOFF_BASE
					.saturating_mul(1_u32 << backoff.failure_count.min(10))
					.min(BACKOFF_MAX)
			},
			|secs| Duration::from_secs(secs).min(BACKOFF_MAX),
		);

		backoff.next_attempt_at = utils::millis_since_unix_epoch()
			.saturating_add(delay.as_millis().try_into().unwrap_or(u64::MAX));

		self.db.senderkey_webpushbackoff.put(key, Json(&backoff));
	}
}
