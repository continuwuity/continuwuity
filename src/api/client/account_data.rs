use axum::extract::State;
use conduwuit::{Err, Result, err};
use conduwuit_service::Services;
use ruma::{
	RoomId, UserId,
	api::client::config::{
		get_global_account_data, get_room_account_data, set_global_account_data,
		set_room_account_data,
	},
	events::{
		AnyGlobalAccountDataEventContent, AnyRoomAccountDataEventContent,
		GlobalAccountDataEventType, RoomAccountDataEventType,
	},
	serde::Raw,
};
use serde::Deserialize;
use serde_json::{json, value::RawValue as RawJsonValue};

use crate::Ruma;

/// # `PUT /_matrix/client/r0/user/{userId}/account_data/{type}`
///
/// Sets some account data for the sender user.
pub(crate) async fn set_global_account_data_route(
	State(services): State<crate::State>,
	body: Ruma<set_global_account_data::v3::Request>,
) -> Result<set_global_account_data::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot set account data for other users.")));
	}

	set_account_data(
		&services,
		None,
		&body.user_id,
		&body.event_type.to_string(),
		body.data.json(),
	)
	.await?;

	Ok(set_global_account_data::v3::Response {})
}

/// # `PUT /_matrix/client/r0/user/{userId}/rooms/{roomId}/account_data/{type}`
///
/// Sets some room account data for the sender user.
pub(crate) async fn set_room_account_data_route(
	State(services): State<crate::State>,
	body: Ruma<set_room_account_data::v3::Request>,
) -> Result<set_room_account_data::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot set account data for other users.")));
	}

	set_account_data(
		&services,
		Some(&body.room_id),
		&body.user_id,
		&body.event_type.to_string(),
		body.data.json(),
	)
	.await?;

	Ok(set_room_account_data::v3::Response {})
}

/// # `GET /_matrix/client/r0/user/{userId}/account_data/{type}`
///
/// Gets some account data for the sender user.
pub(crate) async fn get_global_account_data_route(
	State(services): State<crate::State>,
	body: Ruma<get_global_account_data::v3::Request>,
) -> Result<get_global_account_data::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot get account data of other users.")));
	}

	let account_data: ExtractGlobalEventContent = services
		.account_data
		.get_global(&body.user_id, body.event_type.clone())
		.await
		.map_err(|_| err!(Request(NotFound("Data not found."))))?;

	Ok(get_global_account_data::v3::Response { account_data: account_data.content })
}

/// # `GET /_matrix/client/r0/user/{userId}/rooms/{roomId}/account_data/{type}`
///
/// Gets some room account data for the sender user.
pub(crate) async fn get_room_account_data_route(
	State(services): State<crate::State>,
	body: Ruma<get_room_account_data::v3::Request>,
) -> Result<get_room_account_data::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot get account data of other users.")));
	}

	let account_data: ExtractRoomEventContent = services
		.account_data
		.get_room(&body.room_id, &body.user_id, body.event_type.clone())
		.await
		.map_err(|_| err!(Request(NotFound("Data not found."))))?;

	Ok(get_room_account_data::v3::Response { account_data: account_data.content })
}

async fn set_account_data(
	services: &Services,
	room_id: Option<&RoomId>,
	sender_user: &UserId,
	event_type_s: &str,
	data: &RawJsonValue,
) -> Result {
	if event_type_s == RoomAccountDataEventType::FullyRead.to_cow_str() {
		return Err!(Request(BadJson(
			"This endpoint cannot be used for marking a room as fully read (setting \
			 m.fully_read)"
		)));
	}

	let data: serde_json::Value = serde_json::from_str(data.get())
		.map_err(|e| err!(Request(BadJson(warn!("Invalid JSON provided: {e}")))))?;

	services
		.account_data
		.update(
			room_id,
			sender_user,
			event_type_s.into(),
			&json!({
				"type": event_type_s,
				"content": data,
			}),
		)
		.await
}

#[derive(Deserialize)]
struct ExtractRoomEventContent {
	content: Raw<AnyRoomAccountDataEventContent>,
}

#[derive(Deserialize)]
struct ExtractGlobalEventContent {
	content: Raw<AnyGlobalAccountDataEventContent>,
}
