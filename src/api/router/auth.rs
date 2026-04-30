use std::any::{Any, TypeId};

use conduwuit::{Err, Error, Result, err};
use http::StatusCode;
use ruma::{
	DeviceId, OwnedDeviceId, OwnedServerName, OwnedUserId, UserId,
	api::{
		IncomingRequest,
		auth_scheme::{
			AccessToken, AccessTokenOptional, AppserviceToken, AppserviceTokenOptional,
			AuthScheme, NoAccessToken, NoAuthentication,
		},
		client,
		error::{ErrorKind, UnknownTokenErrorData},
		federation::authentication::ServerSignatures,
	},
	assign,
};
use service::{
	Services,
	server_keys::{PubKeyMap, PubKeys},
	users::AccessTokenStatus,
};

use crate::{router::args::AuthQueryParams, service::appservice::RegistrationInfo};

pub(crate) enum ClientIdentity {
	User {
		sender_user: OwnedUserId,
		sender_device: OwnedDeviceId,
	},
	Appservice {
		sender_user: OwnedUserId,
		sender_device: Option<OwnedDeviceId>,
		appservice_info: Box<RegistrationInfo>,
	},
}

impl ClientIdentity {
	pub(crate) fn sender_user(&self) -> Option<&UserId> {
		match self {
			| Self::User { sender_user, .. } | Self::Appservice { sender_user, .. } =>
				Some(sender_user),
		}
	}

	pub(crate) fn expect_sender_user(&self) -> Result<&UserId> {
		match self {
			| Self::User { sender_user, .. } | Self::Appservice { sender_user, .. } =>
				Ok(sender_user),
		}
	}

	pub(crate) fn sender_device(&self) -> Option<&DeviceId> {
		match self {
			| Self::User { sender_device, .. } => Some(sender_device),
			| Self::Appservice { sender_device, .. } => sender_device.as_deref(),
		}
	}

	pub(crate) fn expect_sender_device(&self) -> Result<&DeviceId> {
		match self {
			| Self::User { sender_device, .. } => Ok(sender_device),
			| Self::Appservice { .. } =>
				Err!(Request(Forbidden("Appservices must masquerade to use this endpoint."))),
		}
	}

	pub(crate) fn appservice_info(&self) -> Option<&RegistrationInfo> {
		match self {
			| Self::User { .. } => None,
			| Self::Appservice { appservice_info, .. } => Some(appservice_info),
		}
	}

	pub(crate) fn is_appservice(&self) -> bool { matches!(self, Self::Appservice { .. }) }
}

pub(crate) trait CheckAuth: AuthScheme {
	type Identity: Send;

	fn authenticate<R: IncomingRequest + Any, B: AsRef<[u8]> + Sync>(
		services: &Services,
		incoming_request: &hyper::Request<B>,
		query: AuthQueryParams,
	) -> impl Future<Output = Result<Self::Identity>> + Send {
		async move {
			let route = TypeId::of::<R>();

			let output = Self::extract_authentication(incoming_request).map_err(|err| {
				err!(Request(Unauthorized(warn!(
					"Failed to extract authorization: {}",
					err.into()
				))))
			})?;

			Self::verify(services, output, incoming_request, query, route).await
		}
	}

	fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> impl Future<Output = Result<Self::Identity>> + Send;
}

impl CheckAuth for ServerSignatures {
	type Identity = OwnedServerName;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Self::Identity> {
		let destination = services.globals.server_name();
		if output
			.destination
			.as_ref()
			.is_some_and(|supplied_destination| supplied_destination != destination)
		{
			return Err!(Request(Unauthorized("Destination mismatch.")));
		}

		let key = services
			.server_keys
			.get_verify_key(&output.origin, &output.key)
			.await
			.map_err(|e| {
				err!(Request(Unauthorized(warn!("Failed to fetch signing keys: {e}"))))
			})?;

		let keys: PubKeys = [(output.key.to_string(), key.key)].into();
		let keys: PubKeyMap = [(output.origin.as_str().into(), keys)].into();

		match output.verify_request(request, destination, &keys) {
			| Ok(()) => {
				if services
					.moderation
					.is_remote_server_forbidden(&output.origin)
				{
					return Err!(Request(Forbidden(
						"You are blocked from federating with this server."
					)));
				}

				Ok(output.origin)
			},
			| Err(err) =>
				Err!(Request(Unauthorized(warn!("Failed to verify X-Matrix header: {err}")))),
		}
	}
}

impl CheckAuth for AccessToken {
	type Identity = ClientIdentity;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		_request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Self::Identity> {
		if output.is_empty() {
			return Err!(Request(Unauthorized("Missing access token.")));
		}
		if let Some((sender_user, sender_device, status)) =
			services.users.find_from_token(&output).await
		{
			// If the token is expired we return a soft logout
			if matches!(status, AccessTokenStatus::Expired) {
				return Err(Error::Request(
					ErrorKind::UnknownToken(
						assign!(UnknownTokenErrorData::new(), { soft_logout: true }),
					),
					"This token has expired".into(),
					StatusCode::UNAUTHORIZED,
				));
			}

			// Locked users can only use /logout and /logout/all
			if services
				.users
				.is_locked(&sender_user)
				.await
				.is_ok_and(std::convert::identity)
			{
				if !(route == TypeId::of::<client::session::logout::v3::Request>()
					|| route == TypeId::of::<client::session::logout_all::v3::Request>())
				{
					return Err!(Request(UserLocked("Your account is locked.")));
				}
			}

			Ok(ClientIdentity::User { sender_user, sender_device })
		} else if let Ok(appservice_info) = services.appservice.find_from_token(&output).await {
			let Ok(sender_user) = query.user_id.clone().map_or_else(
				|| {
					UserId::parse_with_server_name(
						appservice_info.registration.sender_localpart.as_str(),
						services.globals.server_name(),
					)
				},
				UserId::parse,
			) else {
				return Err!(Request(InvalidUsername("Username is invalid.")));
			};

			if !appservice_info.is_user_match(&sender_user) {
				return Err!(Request(Exclusive("User is not in namespace.")));
			}

			// MSC3202/MSC4190: Handle device_id masquerading for appservices.
			// The device_id can be provided via `device_id` or
			// `org.matrix.msc3202.device_id` query parameter.
			let sender_device =
				if let Some(device_id) = query.device_id.as_deref().map(Into::into) {
					// Verify the device exists for this user
					if services
						.users
						.get_device_metadata(&sender_user, device_id)
						.await
						.is_err()
					{
						return Err!(Request(Forbidden(
							"Device does not exist for user or appservice cannot masquerade as \
							 this device."
						)));
					}

					Some(device_id.to_owned())
				} else {
					None
				};

			Ok(ClientIdentity::Appservice {
				sender_user,
				sender_device,
				appservice_info: Box::new(appservice_info),
			})
		} else {
			Err(Error::Request(
				ErrorKind::UnknownToken(UnknownTokenErrorData::new()),
				"Invalid token".into(),
				StatusCode::UNAUTHORIZED,
			))
		}
	}
}

impl CheckAuth for AccessTokenOptional {
	type Identity = Option<ClientIdentity>;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Self::Identity> {
		match output {
			| Some(token) =>
				<AccessToken as CheckAuth>::verify(services, token, request, query, route)
					.await
					.map(Some),
			| None => Ok(None),
		}
	}
}

impl CheckAuth for AppserviceToken {
	type Identity = RegistrationInfo;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		_request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Self::Identity> {
		if output.is_empty() {
			return Err!(Request(Unauthorized("Missing access token.")));
		}
		let Ok(appservice_info) = services.appservice.find_from_token(&output).await else {
			return Err!(Request(Unauthorized("Invalid appservice token.")));
		};

		Ok(appservice_info)
	}
}

impl CheckAuth for AppserviceTokenOptional {
	type Identity = Option<RegistrationInfo>;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Self::Identity> {
		match output {
			| Some(token) =>
				<AppserviceToken as CheckAuth>::verify(services, token, request, query, route)
					.await
					.map(Some),
			| None => Ok(None),
		}
	}
}

impl CheckAuth for NoAuthentication {
	type Identity = ();

	async fn verify<B: AsRef<[u8]> + Sync>(
		_services: &Services,
		_output: Self::Output,
		_request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Self::Identity> {
		Ok(())
	}
}

impl CheckAuth for NoAccessToken {
	type Identity = Option<ClientIdentity>;

	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		_output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Self::Identity> {
		// We handle these the same as AccessTokenOptional
		let token = AccessTokenOptional::extract_authentication(request).map_err(|err| {
			err!(Request(Unauthorized(warn!("Failed to extract authorization: {}", err))))
		})?;

		// Check special access restrictions
		if (route == TypeId::of::<client::profile::get_avatar_url::v3::Request>()
			|| route == TypeId::of::<client::profile::get_display_name::v3::Request>()
			|| route == TypeId::of::<client::profile::get_profile_field::v3::Request>()
			|| route == TypeId::of::<client::profile::get_profile::v3::Request>())
			&& services.config.require_auth_for_profile_requests
			&& token.is_none()
		{
			return Err!(Request(Unauthorized(
				"This server requires authentication to access user profiles."
			)));
		}

		<AccessTokenOptional as CheckAuth>::verify(services, token, request, query, route).await
	}
}
