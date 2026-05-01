use std::any::{Any, TypeId};

use conduwuit::{Err, Result, err};
use ruma::{
	OwnedDeviceId, OwnedServerName, OwnedUserId, UserId,
	api::{
		IncomingRequest,
		auth_scheme::{
			AccessToken, AccessTokenOptional, AppserviceToken, AppserviceTokenOptional,
			AuthScheme, NoAccessToken, NoAuthentication,
		},
		federation::authentication::ServerSignatures,
	},
};
use service::{
	Services,
	server_keys::{PubKeyMap, PubKeys},
};

use crate::{router::args::AuthQueryParams, service::appservice::RegistrationInfo};

#[derive(Default)]
pub(super) struct Auth {
	pub(super) origin: Option<OwnedServerName>,
	pub(super) sender_user: Option<OwnedUserId>,
	pub(super) sender_device: Option<OwnedDeviceId>,
	pub(super) appservice_info: Option<RegistrationInfo>,
}

pub(super) trait CheckAuth: AuthScheme {
	fn authenticate<R: IncomingRequest + Any, B: AsRef<[u8]> + Sync>(
		services: &Services,
		incoming_request: &hyper::Request<B>,
		query: AuthQueryParams,
	) -> impl Future<Output = Result<Auth>> + Send {
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
	) -> impl Future<Output = Result<Auth>> + Send;
}

impl CheckAuth for ServerSignatures {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Auth> {
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
			| Ok(()) => Ok(Auth {
				origin: Some(output.origin.clone()),
				..Default::default()
			}),
			| Err(err) => {
				Err!(Request(Unauthorized(warn!("Failed to verify X-Matrix header: {err}"))))
			},
		}
	}
}

impl CheckAuth for AccessToken {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		_request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Auth> {
		// Check for appservice tokens first

		let (sender_user, sender_device, appservice_info) = {
			if let Ok((sender_user, sender_device)) =
				services.users.find_from_token(&output).await
			{
				// Locked users can only use /logout and /logout/all
				if services
					.users
					.is_locked(&sender_user)
					.await
					.is_ok_and(std::convert::identity)
				{
					if !(route == TypeId::of::<ruma::api::client::session::logout::v3::Request>()
						|| route
							== TypeId::of::<ruma::api::client::session::logout_all::v3::Request>(
							)) {
						return Err!(Request(Unauthorized("Your account is locked.")));
					}
				}

				(Some(sender_user), Some(sender_device), None)
			} else if let Ok(appservice_info) = services.appservice.find_from_token(&output).await
			{
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
								"Device does not exist for user or appservice cannot masquerade \
								 as this device."
							)));
						}

						Some(device_id.to_owned())
					} else {
						None
					};

				(Some(sender_user), sender_device, Some(appservice_info))
			} else {
				return Err!(Request(Unauthorized("Invalid access token.")));
			}
		};

		Ok(Auth {
			sender_user,
			sender_device,
			appservice_info,
			..Default::default()
		})
	}
}

impl CheckAuth for AccessTokenOptional {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Auth> {
		match output {
			| Some(token) =>
				<AccessToken as CheckAuth>::verify(services, token, request, query, route).await,
			| None => Ok(Auth::default()),
		}
	}
}

impl CheckAuth for AppserviceToken {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		_request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Auth> {
		let Ok(appservice_info) = services.appservice.find_from_token(&output).await else {
			return Err!(Request(Unauthorized("Invalid appservice token.")));
		};

		Ok(Auth {
			appservice_info: Some(appservice_info),
			..Default::default()
		})
	}
}

impl CheckAuth for AppserviceTokenOptional {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Auth> {
		match output {
			| Some(token) =>
				<AppserviceToken as CheckAuth>::verify(services, token, request, query, route)
					.await,
			| None => Ok(Auth::default()),
		}
	}
}

impl CheckAuth for NoAuthentication {
	async fn verify<B: AsRef<[u8]> + Sync>(
		_services: &Services,
		_output: Self::Output,
		_request: &hyper::Request<B>,
		_query: AuthQueryParams,
		_route: TypeId,
	) -> Result<Auth> {
		Ok(Auth::default())
	}
}

impl CheckAuth for NoAccessToken {
	async fn verify<B: AsRef<[u8]> + Sync>(
		services: &Services,
		_output: Self::Output,
		request: &hyper::Request<B>,
		query: AuthQueryParams,
		route: TypeId,
	) -> Result<Auth> {
		// We handle these the same as AccessTokenOptional
		let token = AccessTokenOptional::extract_authentication(request).map_err(|err| {
			err!(Request(Unauthorized(warn!("Failed to extract authorization: {}", err))))
		})?;

		<AccessTokenOptional as CheckAuth>::verify(services, token, request, query, route).await
	}
}
