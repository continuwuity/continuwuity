mod webpush;

use std::{fmt::Debug, mem, sync::Arc};

use bytes::BytesMut;
use conduwuit::utils::response::LimitReadExt;
use conduwuit_core::{
	Err, Event, Result, Server, debug_warn, err, trace,
	utils::{stream::TryIgnore, string_from_bytes},
	warn,
};
use conduwuit_database::{Deserialized, Ignore, Interfix, Json, Map};
use futures::{Stream, StreamExt};
use ipaddress::IPAddress;
use ruma::{
	DeviceId, OwnedDeviceId, RoomId, UInt, UserId,
	api::{
		IncomingResponseExt, OutgoingRequest, OutgoingRequestExt,
		auth_scheme::NoAuthentication,
		path_builder::SinglePath,
		push_gateway::send_event_notification::{
			self,
			v1::{Device, Notification, NotificationCounts, NotificationPriority},
		},
	},
	events::{AnySyncTimelineEvent, TimelineEventType, room::power_levels::RoomPowerLevels},
	push::{
		Action, HighlightTweakValue, PushConditionPowerLevelsCtx, PushConditionRoomCtx,
		PushFormat, Ruleset, Tweak,
	},
	serde::{JsonObject, Raw},
	uint,
};
use ruminuwuity::pushers::{Pusher, PusherKind, set_pusher::v3::PusherAction};

pub use self::webpush::ActivationOutcome;
use self::webpush::Vapid;
use crate::{Dep, client, config, globals, rooms, sending, users};

pub struct Service {
	db: Data,
	services: Services,
	me: std::sync::Weak<Self>,
	vapid: Option<Vapid>,
}

struct Services {
	server: Arc<Server>,
	globals: Dep<globals::Service>,
	config: Dep<config::Service>,
	client: Dep<client::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	users: Dep<users::Service>,
	sending: Dep<sending::Service>,
}

struct Data {
	senderkey_pusher: Arc<Map>,
	senderkey_webpushbackoff: Arc<Map>,
	senderkey_webpushstate: Arc<Map>,
	pushkey_deviceid: Arc<Map>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let vapid = args
			.server
			.config
			.well_known
			.support_page
			.is_some()
			.then(|| Vapid::init(args.db))
			.transpose()?;

		Ok(Arc::new_cyclic(|me| Self {
			db: Data {
				senderkey_pusher: args.db["senderkey_pusher"].clone(),
				senderkey_webpushbackoff: args.db["senderkey_webpushbackoff"].clone(),
				senderkey_webpushstate: args.db["senderkey_webpushstate"].clone(),
				pushkey_deviceid: args.db["pushkey_deviceid"].clone(),
			},
			services: Services {
				server: args.server.clone(),
				globals: args.depend::<globals::Service>("globals"),
				client: args.depend::<client::Service>("client"),
				config: args.depend::<config::Service>("config"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				users: args.depend::<users::Service>("users"),
				sending: args.depend::<sending::Service>("sending"),
			},
			me: me.clone(),
			vapid,
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub async fn set_pusher(
		&self,
		sender: &UserId,
		sender_device: &DeviceId,
		action: &PusherAction,
	) -> Result<bool> {
		let pusher = match action {
			| PusherAction::Post(pusher) => pusher,
			| PusherAction::Delete(ids) => {
				self.delete_pusher(sender, ids.pushkey.as_str()).await;
				return Ok(false);
			},
		};

		let pushkey = pusher.ids.pushkey.as_str();
		if pushkey.len() > 512 {
			return Err!(Request(InvalidParam(
				"Push key length cannot be greater than 512 bytes."
			)));
		}

		if pusher.ids.app_id.as_str().len() > 64 {
			return Err!(Request(InvalidParam("App ID length cannot be greater than 64 bytes.")));
		}

		match &pusher.kind {
			| PusherKind::Http(http) => {
				self.validate_push_url(&http.url, false)?;
			},
			| PusherKind::WebPush(data) => {
				if self.vapid.is_none() {
					return Err!(Request(InvalidParam(
						"Web push is not enabled on this server."
					)));
				}

				self.validate_push_url(&data.url, true)?;
				webpush::decode_pushkey(pushkey)?;
				webpush::decode_auth(&data.auth)?;

				if let Some(default_payload) = data.data.get("default_payload") {
					if serde_json::to_vec(default_payload)?.len() > webpush::MAX_DEFAULT_PAYLOAD {
						return Err!(Request(TooLarge(
							"Default payload cannot be greater than {} bytes.",
							webpush::MAX_DEFAULT_PAYLOAD
						)));
					}
				}
			},
			| PusherKind::Email(_) | PusherKind::Custom { .. } => (),
		}

		let key = (sender, pushkey);
		let previous: Option<Pusher> = self
			.db
			.senderkey_pusher
			.qry(&key)
			.await
			.deserialized()
			.map_err(|e| {
				if !e.is_not_found() {
					warn!(%pushkey, "Replacing a stored pusher that could not be read: {e}");
				}
			})
			.ok();
		let needs_activation = self
			.webpush_needs_activation(sender, pusher, previous.as_ref())
			.await;

		if needs_activation {
			self.begin_webpush_activation(sender, pusher);
		} else if pusher.kind.as_webpush().is_none() {
			self.db.senderkey_webpushstate.del(key);
			self.db.senderkey_webpushbackoff.del(key);
		}

		self.db.senderkey_pusher.put(key, Json(pusher));
		self.db.pushkey_deviceid.insert(pushkey, sender_device);

		Ok(needs_activation)
	}

	pub async fn delete_pusher(&self, sender: &UserId, pushkey: &str) {
		let key = (sender, pushkey);
		self.db.senderkey_pusher.del(key);
		self.db.senderkey_webpushbackoff.del(key);
		self.db.senderkey_webpushstate.del(key);
		self.db.pushkey_deviceid.remove(pushkey);

		self.services
			.sending
			.cleanup_events(None, Some(sender), Some(pushkey))
			.await
			.ok();
	}

	pub async fn get_pusher_device(&self, pushkey: &str) -> Result<OwnedDeviceId> {
		self.db.pushkey_deviceid.get(pushkey).await.deserialized()
	}

	pub async fn get_pusher(&self, sender: &UserId, pushkey: &str) -> Result<Pusher> {
		let senderkey = (sender, pushkey);
		self.db
			.senderkey_pusher
			.qry(&senderkey)
			.await
			.deserialized()
	}

	pub async fn get_pushers(&self, sender: &UserId) -> Vec<Pusher> {
		let prefix = (sender, Interfix);
		self.db
			.senderkey_pusher
			.stream_prefix(&prefix)
			.ignore_err()
			.map(|(_, pusher): (Ignore, Pusher)| pusher)
			.collect()
			.await
	}

	pub fn get_pushkeys<'a>(
		&'a self,
		sender: &'a UserId,
	) -> impl Stream<Item = &'a str> + Send + 'a {
		let prefix = (sender, Interfix);
		self.db
			.senderkey_pusher
			.keys_prefix(&prefix)
			.ignore_err()
			.map(|(_, pushkey): (Ignore, &str)| pushkey)
	}

	/// Checks that a pusher URL is well-formed and not pointing at a forbidden
	/// address.
	fn validate_push_url(&self, url: &str, require_https: bool) -> Result<url::Url> {
		let parsed = url::Url::parse(url).map_err(|e| {
			err!(Request(InvalidParam(warn!(%url, "Pusher URL is not a valid URL: {e}"))))
		})?;

		let scheme = parsed.scheme().to_lowercase();
		let allowed: &[&str] = if require_https { &["https"] } else { &["http", "https"] };
		if !allowed.contains(&scheme.as_str()) {
			return Err!(Request(InvalidParam(warn!(
				%url,
				"Pusher URL scheme {scheme} is not allowed"
			))));
		}

		if let Some(host) = parsed.host_str() {
			if let Ok(ip) = IPAddress::parse(host) {
				if !self.services.client.valid_cidr_range(&ip) {
					return Err!(Request(InvalidParam(warn!(
						%url,
						"Pusher URL is a forbidden remote address"
					))));
				}
			}
		}

		Ok(parsed)
	}

	#[tracing::instrument(skip(self, dest, request))]
	pub async fn send_request<T>(&self, dest: &str, request: T) -> Result<T::IncomingResponse>
	where
		T: OutgoingRequest<Authentication = NoAuthentication, PathBuilder = SinglePath>
			+ Debug
			+ Send,
	{
		let dest = dest.replace(self.services.globals.notification_push_path(), "");
		trace!("Push gateway destination: {dest}");

		let http_request = request
			.try_into_http_request::<BytesMut>(&dest, (), ())
			.map_err(|e| {
				err!(BadServerResponse(warn!(
					"Failed to find destination {dest} for push gateway: {e}"
				)))
			})?
			.map(BytesMut::freeze);

		let (status, version, headers, body) = self
			.execute_pusher_request(
				&self.services.client.pusher,
				reqwest::Request::try_from(http_request)?,
			)
			.await?;

		if !status.is_success() {
			debug_warn!("Push gateway response body: {:?}", string_from_bytes(&body));
			return Err!(BadServerResponse(warn!(
				"Push gateway {dest} returned unsuccessful HTTP response: {status}"
			)));
		}

		let mut builder = http::Response::builder().status(status).version(version);
		*builder
			.headers_mut()
			.expect("http::response::Builder is usable") = headers;

		let (parts, body) = builder
			.body(body)
			.expect("reqwest body is valid http body")
			.into_parts();

		T::IncomingResponse::try_from_http_response(http::Response::from_parts(
			parts,
			body.as_ref(),
		))
		.map_err(|e| {
			err!(BadServerResponse(warn!("Push gateway {dest} returned invalid response: {e}")))
		})
	}

	/// Executes a request, checking the destination address on the way out and
	/// on the way in. The status is returned as-is; callers decide which codes
	/// are failures.
	async fn execute_pusher_request(
		&self,
		client: &reqwest::Client,
		request: reqwest::Request,
	) -> Result<(http::StatusCode, http::Version, http::HeaderMap, Vec<u8>)> {
		if let Some(url_host) = request.url().host_str() {
			trace!("Checking request URL for IP");
			if let Ok(ip) = IPAddress::parse(url_host) {
				if !self.services.client.valid_cidr_range(&ip) {
					return Err!(BadServerResponse("Not allowed to send requests to this IP"));
				}
			}
		}

		let dest = request.url().clone();
		let mut response = client.execute(request).await.map_err(|e| {
			err!(BadServerResponse(warn!(%dest, "Could not send request to pusher: {e}")))
		})?;

		trace!("Checking response destination's IP");
		if let Some(remote_addr) = response.remote_addr() {
			if let Ok(ip) = IPAddress::parse(remote_addr.ip().to_string()) {
				if !self.services.client.valid_cidr_range(&ip) {
					return Err!(BadServerResponse("Not allowed to send requests to this IP"));
				}
			}
		}

		let status = response.status();
		let version = response.version();
		let mut headers = http::HeaderMap::new();
		mem::swap(response.headers_mut(), &mut headers);

		let body = response
			.limit_read(
				self.services
					.config
					.max_request_size
					.try_into()
					.expect("usize fits into u64"),
			)
			.await?;

		Ok((status, version, headers, body))
	}

	#[tracing::instrument(skip(self, user, unread, pusher, ruleset, event))]
	pub async fn send_push_notice<E>(
		&self,
		user: &UserId,
		unread: UInt,
		pusher: &Pusher,
		ruleset: Ruleset,
		event: &E,
	) -> Result
	where
		E: Event + Send + Sync,
		for<'a> &'a E: Event + Send,
	{
		let mut notify = None;
		let mut tweaks = Vec::new();
		let Some(room_id) = event.room_id() else {
			// Only v12+ create events have no room ID
			return Ok(());
		};

		let power_levels = self
			.services
			.state_accessor
			.get_room_power_levels(room_id)
			.await;

		let serialized = event.to_format();
		for action in self
			.get_actions(user, &ruleset, power_levels.clone(), &serialized, room_id)
			.await
		{
			let n = match action {
				| Action::Notify => true,
				| Action::SetTweak(tweak) => {
					tweaks.push(tweak.clone());
					continue;
				},
				| _ => false,
			};

			if notify.is_some() {
				return Err!(Database(
					r#"Malformed pushrule contains more than one of these actions: ["dont_notify", "notify", "coalesce"]"#
				));
			}

			notify = Some(n);
		}

		if notify == Some(true) {
			self.send_notice(user, unread, pusher, tweaks, event)
				.await?;
		}
		// Else the event triggered no actions

		Ok(())
	}

	pub async fn push_joined_count(&self, room_id: &RoomId) -> UInt {
		self.services
			.state_cache
			.room_joined_count(room_id)
			.await
			.unwrap_or(1)
			.try_into()
			.unwrap_or_else(|_| uint!(0))
	}

	#[tracing::instrument(skip(self, user), level = "debug")]
	pub async fn push_condition_ctx(
		&self,
		user: &UserId,
		power_levels: RoomPowerLevels,
		room_id: &RoomId,
		room_joined_count: UInt,
	) -> PushConditionRoomCtx {
		let power_levels = PushConditionPowerLevelsCtx::from(power_levels);

		let user_display_name = self
			.services
			.users
			.displayname(user)
			.await
			.unwrap_or_else(|_| user.localpart().to_owned());

		PushConditionRoomCtx::new(
			room_id.to_owned(),
			room_joined_count,
			user.to_owned(),
			user_display_name,
		)
		.with_power_levels(power_levels)
	}

	#[tracing::instrument(skip(self, user, ruleset, pdu), level = "debug")]
	pub async fn get_actions<'a>(
		&self,
		user: &UserId,
		ruleset: &'a Ruleset,
		power_levels: RoomPowerLevels,
		pdu: &Raw<AnySyncTimelineEvent>,
		room_id: &RoomId,
	) -> &'a [Action] {
		let room_joined_count = self.push_joined_count(room_id).await;
		let ctx = self
			.push_condition_ctx(user, power_levels, room_id, room_joined_count)
			.await;

		ruleset.get_actions(pdu, &ctx).await
	}

	#[tracing::instrument(skip(self, unread, pusher, tweaks, event))]
	async fn send_notice<E>(
		&self,
		user: &UserId,
		unread: UInt,
		pusher: &Pusher,
		tweaks: Vec<Tweak>,
		event: &E,
	) -> Result
	where
		E: Event + Send + Sync,
	{
		// TODO: email
		match &pusher.kind {
			| PusherKind::Http(http) => {
				self.validate_push_url(&http.url, false)?;

				let notify = self
					.build_notification(
						pusher,
						&http.data,
						http.format.as_ref(),
						&tweaks,
						event,
						unread,
					)
					.await;

				self.send_request(&http.url, send_event_notification::v1::Request::new(notify))
					.await?;

				Ok(())
			},
			| PusherKind::WebPush(data) => {
				let notify = self
					.build_notification(
						pusher,
						&data.data,
						data.format.as_ref(),
						&tweaks,
						event,
						unread,
					)
					.await;

				self.send_webpush_notice(user, pusher, data, notify).await
			},
			| PusherKind::Email(_) | PusherKind::Custom { .. } => Ok(()),
		}
	}

	async fn build_notification<E>(
		&self,
		pusher: &Pusher,
		data: &JsonObject,
		format: Option<&PushFormat>,
		tweaks: &[Tweak],
		event: &E,
		unread: UInt,
	) -> Notification
	where
		E: Event + Send + Sync,
	{
		// TODO (timo): can pusher/devices have conflicting formats
		let event_id_only = format == Some(&PushFormat::EventIdOnly);

		let mut device = Device::new(pusher.ids.app_id.clone(), pusher.ids.pushkey.clone());
		device.data.data.clone_from(data);
		device.data.format = format.cloned();

		// Tweaks are only added if the format is NOT event_id_only
		if !event_id_only {
			device.tweaks = tweaks.to_owned();
		}

		let mut notify = Notification::new(vec![device]);

		notify.event_id = Some(event.event_id().to_owned());
		notify.room_id = event.room_id().map(ToOwned::to_owned);
		if data.get("org.matrix.msc4076.disable_badge_count").is_none()
			&& data.get("disable_badge_count").is_none()
		{
			notify.counts = NotificationCounts::new(unread, uint!(0));
		} else {
			// counts will not be serialised if it's the default (0, 0)
			// skip_serializing_if = "NotificationCounts::is_default"
			notify.counts = NotificationCounts::default();
		}

		if !event_id_only {
			if *event.kind() == TimelineEventType::RoomEncrypted
				|| tweaks.iter().any(|t| {
					matches!(t, Tweak::Highlight(HighlightTweakValue::Yes) | Tweak::Sound(_))
				}) {
				notify.prio = NotificationPriority::High;
			} else {
				notify.prio = NotificationPriority::Low;
			}
			notify.sender = Some(event.sender().to_owned());
			notify.event_type = Some(event.kind().to_owned());
			notify.content = serde_json::value::to_raw_value(event.content()).ok();

			if *event.kind() == TimelineEventType::RoomMember {
				notify.user_is_target = event.state_key() == Some(event.sender().as_str());
			}

			notify.sender_display_name =
				self.services.users.displayname(event.sender()).await.ok();

			if let Some(room_id) = event.room_id() {
				notify.room_name = self.services.state_accessor.get_name(room_id).await.ok();

				notify.room_alias = self
					.services
					.state_accessor
					.get_canonical_alias(room_id)
					.await
					.ok();
			}
		}

		notify
	}
}
