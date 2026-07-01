use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{Err, Result};
use conduwuit_service::Services;
use ruma::{
	UserId,
	api::{
		client::profile::{
			PropagateTo, delete_profile_field, get_profile, get_profile_field, set_profile_field,
		},
		federation,
	},
	assign,
	profile::{ProfileFieldName, ProfileFieldValue},
};
use serde_json::Value;
use service::users::ProfileFieldChange;

use crate::Ruma;

/// # `GET /_matrix/client/v3/profile/{userId}`
///
/// Returns the user's profile information.
///
/// - If user is on another server and we do not have a local copy already,
///   fetch profile over federation.
pub(crate) async fn get_profile_route(
	State(services): State<crate::State>,
	body: Ruma<get_profile::v3::Request>,
) -> Result<get_profile::v3::Response> {
	let Some(profile) = fetch_full_profile(&services, &body.user_id).await else {
		return Err!(Request(NotFound("This user's profile could not be fetched.")));
	};

	Ok(get_profile::v3::Response::from_iter(profile))
}

pub(crate) async fn get_profile_field_route(
	State(services): State<crate::State>,
	body: Ruma<get_profile_field::v3::Request>,
) -> Result<get_profile_field::v3::Response> {
	let value = fetch_profile_field(&services, &body.user_id, body.field.clone()).await?;

	Ok(assign!(get_profile_field::v3::Response::default(), { value }))
}

pub(crate) async fn set_profile_field_route(
	State(services): State<crate::State>,
	body: Ruma<set_profile_field::v3::Request>,
) -> Result<set_profile_field::v3::Response> {
	if body.user_id != body.identity.expect_sender_user()?
		&& !(body.identity.is_appservice()
			|| services
				.admin
				.user_is_admin(body.identity.expect_sender_user()?)
				.await)
	{
		return Err!(Request(Forbidden("You may not change other users' profile data.")));
	}

	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("You may not change a remote user's profile data.")));
	}

	if services
		.oidc
		.restricted_profile_fields()
		.contains(&body.value.field_name())
	{
		return Err!(Request(Forbidden(
			"This profile field is controlled by your identity provider."
		)));
	}

	services
		.users
		.set_profile_field(
			&body.user_id,
			ProfileFieldChange::Set(body.value.clone()),
			body.propagate_to.clone(),
		)
		.await?;

	Ok(set_profile_field::v3::Response::new())
}

pub(crate) async fn delete_profile_field_route(
	State(services): State<crate::State>,
	body: Ruma<delete_profile_field::v3::Request>,
) -> Result<delete_profile_field::v3::Response> {
	if body.user_id != body.identity.expect_sender_user()?
		&& !(body.identity.is_appservice()
			|| services
				.admin
				.user_is_admin(body.identity.expect_sender_user()?)
				.await)
	{
		return Err!(Request(Forbidden("You may not change other users' profile data.")));
	}

	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("You may not change a remote user's profile data.")));
	}

	if services
		.oidc
		.restricted_profile_fields()
		.contains(&body.field)
	{
		return Err!(Request(Forbidden(
			"This profile field is controlled by your identity provider."
		)));
	}

	services
		.users
		.set_profile_field(
			&body.user_id,
			ProfileFieldChange::Delete(body.field.clone()),
			body.propagate_to.clone(),
		)
		.await?;

	Ok(delete_profile_field::v3::Response::new())
}

async fn fetch_full_profile(
	services: &Services,
	user_id: &UserId,
) -> Option<BTreeMap<String, Value>> {
	// If the user exists locally, fetch their local profile
	if services.users.status(user_id).await.is_found() {
		return Some(services.users.get_local_profile(user_id).await);
	}

	// Otherwise ask their homeserver
	let Ok(response) = services
		.sending
		.send_federation_request(
			user_id.server_name(),
			federation::query::get_profile_information::v1::Request::new(user_id.to_owned()),
		)
		.await
	else {
		return None;
	};

	// Update our local copies of their profile fields
	services.users.clear_profile(user_id).await;

	for (field, value) in response.iter() {
		let Ok(value) = ProfileFieldValue::new(field, value.to_owned()) else {
			// Skip malformed fields
			continue;
		};

		let _ = services
			.users
			.set_profile_field(user_id, ProfileFieldChange::Set(value), PropagateTo::None)
			.await;
	}

	Some(BTreeMap::from_iter(response))
}

async fn fetch_profile_field(
	services: &Services,
	user_id: &UserId,
	field: ProfileFieldName,
) -> Result<Option<ProfileFieldValue>> {
	// If the user exists locally, fetch their local profile field
	if services.globals.user_is_local(user_id) {
		return Ok(services.users.get_local_profile_field(user_id, field).await);
	}

	// Otherwise ask their homeserver
	let Ok(response) = services
		.sending
		.send_federation_request(
			user_id.server_name(),
			assign!(federation::query::get_profile_information::v1::Request::new(user_id.to_owned()), {
				field: Some(field.clone())
			}),
		)
		.await
	else {
		return Err!(Request(NotFound(
			"User's homeserver could not provide this profile field."
		)));
	};

	if let Some(value) = response.get(field.as_str()).map(ToOwned::to_owned) {
		if let Ok(value) = ProfileFieldValue::new(field.as_str(), value) {
			let _ = services
				.users
				.set_profile_field(
					user_id,
					ProfileFieldChange::Set(value.clone()),
					PropagateTo::None,
				)
				.await;

			Ok(Some(value))
		} else {
			Err!(Request(Unknown(
				"User's homeserver returned malformed data for this profile field."
			)))
		}
	} else {
		let _ = services
			.users
			.set_profile_field(user_id, ProfileFieldChange::Delete(field), PropagateTo::None)
			.await;

		Ok(None)
	}
}
