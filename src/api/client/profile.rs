use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use conduwuit_service::Services;
use futures::StreamExt;
use ruma::{
	UserId,
	api::{
		client::profile::{
			delete_profile_field, get_profile, get_profile_field, set_profile_field,
		},
		federation,
	},
	assign,
	events::room::member::{MembershipState, RoomMemberEventContent},
	presence::PresenceState,
	profile::{ProfileFieldName, ProfileFieldValue},
};
use serde_json::{Value, to_value};

use crate::Ruma;

/// # `GET /_matrix/client/v3/profile/{userId}`
///
/// Returns the displayname, avatar_url, blurhash, and custom profile fields of
/// the user.
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
	if body.user_id != body.sender_user()
		&& !(body.appservice_info.is_some()
			|| services.admin.user_is_admin(body.sender_user()).await)
	{
		return Err!(Request(Forbidden("You may not change other users' profile data.")));
	}

	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("You may not change a remote user's profile data.")));
	}

	set_profile_field(&services, &body.user_id, ProfileFieldChange::Set(body.value.clone()))
		.await;

	Ok(set_profile_field::v3::Response::new())
}

pub(crate) async fn delete_profile_field_route(
	State(services): State<crate::State>,
	body: Ruma<delete_profile_field::v3::Request>,
) -> Result<delete_profile_field::v3::Response> {
	if body.user_id != body.sender_user()
		&& !(body.appservice_info.is_some()
			|| services.admin.user_is_admin(body.sender_user()).await)
	{
		return Err!(Request(Forbidden("You may not change other users' profile data.")));
	}

	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("You may not change a remote user's profile data.")));
	}

	set_profile_field(&services, &body.user_id, ProfileFieldChange::Delete(body.field.clone()))
		.await;

	Ok(delete_profile_field::v3::Response::new())
}

async fn fetch_full_profile(
	services: &Services,
	user_id: &UserId,
) -> Option<BTreeMap<String, Value>> {
	// If the user exists locally, fetch their local profile
	if services.users.exists(user_id).await {
		return Some(get_local_profile(services, user_id).await);
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

		set_profile_field(services, user_id, ProfileFieldChange::Set(value)).await;
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
		return Ok(get_local_profile_field(services, user_id, field).await);
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
			set_profile_field(services, user_id, ProfileFieldChange::Set(value.clone())).await;

			Ok(Some(value))
		} else {
			Err!(Request(Unknown(
				"User's homeserver returned malformed data for this profile field."
			)))
		}
	} else {
		set_profile_field(services, user_id, ProfileFieldChange::Delete(field)).await;

		Ok(None)
	}
}

pub(crate) async fn get_local_profile(
	services: &Services,
	user_id: &UserId,
) -> BTreeMap<String, Value> {
	let mut profile = BTreeMap::new();

	// Get displayname and avatar_url independently because `all_profile_keys`
	// doesn't include them
	for field in [ProfileFieldName::AvatarUrl, ProfileFieldName::DisplayName] {
		let key = field.as_str().to_owned();

		if let Some(value) = get_local_profile_field(services, user_id, field).await {
			profile.insert(key, value.value().into_owned());
		}
	}

	// Insert all other profile fields
	let mut all_fields = services.users.all_profile_keys(user_id);

	while let Some((key, value)) = all_fields.next().await {
		profile.insert(key, value);
	}

	profile
}

pub(crate) async fn get_local_profile_field(
	services: &Services,
	user_id: &UserId,
	field: ProfileFieldName,
) -> Option<ProfileFieldValue> {
	let value = match field.clone() {
		| ProfileFieldName::AvatarUrl => services
			.users
			.avatar_url(user_id)
			.await
			.ok()
			.map(to_value)
			.transpose()
			.expect("converting avatar url to value should succeed"),
		| ProfileFieldName::DisplayName => services
			.users
			.displayname(user_id)
			.await
			.ok()
			.map(to_value)
			.transpose()
			.expect("converting displayname to value should succeed"),
		| other => services
			.users
			.profile_key(user_id, other.as_str())
			.await
			.ok(),
	}?;

	Some(
		ProfileFieldValue::new(field.as_str(), value)
			.expect("local profile field should be valid"),
	)
}

enum ProfileFieldChange {
	Set(ProfileFieldValue),
	Delete(ProfileFieldName),
}

impl ProfileFieldChange {
	fn field_name(&self) -> ProfileFieldName {
		match self {
			| &Self::Delete(ref name) => name.clone(),
			| &Self::Set(ref value) => value.field_name(),
		}
	}

	fn value(&self) -> Option<Value> {
		if let Self::Set(value) = self {
			Some(value.value().into_owned())
		} else {
			None
		}
	}
}

async fn set_profile_field(services: &Services, user_id: &UserId, change: ProfileFieldChange) {
	let field_name = change.field_name();

	match change {
		| ProfileFieldChange::Set(ProfileFieldValue::DisplayName(displayname)) => {
			services
				.users
				.set_displayname(user_id, Some(displayname).filter(|dn| !dn.is_empty()));
		},
		| ProfileFieldChange::Set(ProfileFieldValue::AvatarUrl(avatar_url)) => {
			services
				.users
				.set_avatar_url(user_id, Some(avatar_url).filter(|av| av.is_valid()));
		},
		| ProfileFieldChange::Delete(ProfileFieldName::DisplayName) => {
			services.users.set_displayname(user_id, None);
		},
		| ProfileFieldChange::Delete(ProfileFieldName::AvatarUrl) => {
			services.users.set_avatar_url(user_id, None);
		},
		| other =>
			if other.field_name().as_str() == "blurhash" {
				if let Some(Value::String(blurhash)) = other.value() {
					services.users.set_blurhash(user_id, Some(blurhash));
				} else {
					services.users.set_blurhash(user_id, None);
				}
			} else {
				services.users.set_profile_key(
					user_id,
					other.field_name().as_str(),
					other.value(),
				);
			},
	}

	// If the user is local and changed their displayname or avatar_url, update it
	// in all their joined rooms
	if matches!(field_name, ProfileFieldName::AvatarUrl | ProfileFieldName::DisplayName)
		&& services.users.is_active_local(user_id).await
	{
		let displayname = services.users.displayname(user_id).await.ok();
		let avatar_url = services.users.avatar_url(user_id).await.ok();
		let membership_content = assign!(
			RoomMemberEventContent::new(MembershipState::Join), { displayname, avatar_url }
		);

		let mut all_joined_rooms = services.rooms.state_cache.rooms_joined(user_id);

		while let Some(room_id) = all_joined_rooms.next().await {
			let state_lock = services.rooms.state.mutex.lock(room_id.as_str()).await;

			let _ = services
				.rooms
				.timeline
				.build_and_append_pdu(
					PartialPdu::state(user_id.to_string(), &membership_content),
					user_id,
					Some(&room_id),
					&state_lock,
				)
				.await;
		}

		if services.config.allow_local_presence {
			// Send a presence EDU to indicate the profile changed
			let _ = services
				.presence
				.ping_presence(user_id, &PresenceState::Online)
				.await;
		}
	}
}
