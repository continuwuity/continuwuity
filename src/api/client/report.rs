use std::{fmt::Write as _, time::Duration};

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{Err, Event, Result, debug_info, info, matrix::pdu::PduEvent, utils::ReadyExt};
use conduwuit_service::Services;
use ruma::{
	EventId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId,
	api::client::{
		report_user,
		room::{report_content, report_room},
	},
	events::{Mentions, room::message::RoomMessageEventContent},
};
use tokio::time::sleep;

use crate::Ruma;

struct Report {
	sender: OwnedUserId,
	room_id: Option<OwnedRoomId>,
	event_id: Option<OwnedEventId>,
	user_id: Option<OwnedUserId>,
	report_type: String,
	reason: Option<String>,
}

/// # `POST /_matrix/client/v3/rooms/{roomId}/report`
///
/// Reports an abusive room to homeserver admins
#[tracing::instrument(skip_all, fields(%client), name = "report_room", level = "info")]
pub(crate) async fn report_room_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<report_room::v3::Request>,
) -> Result<report_room::v3::Response> {
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	if body.reason.as_ref().is_some_and(|s| s.len() > 750) {
		return Err!(Request(
			InvalidParam("Reason too long, should be 750 characters or fewer",)
		));
	}

	delay_response().await;

	// We log this early in case the room ID does actually exist, in which case
	// admins who scan their logs can see the report and choose to investigate at
	// their discretion.
	info!(
		"Received room report by user {sender_user} for room {} with reason: \"{}\"",
		body.room_id,
		body.reason.as_deref().unwrap_or("")
	);

	if !services
		.rooms
		.state_cache
		.server_in_room(&services.server.name, &body.room_id)
		.await
	{
		return Err!(Request(NotFound(
			"Room does not exist to us, no local users have joined at all"
		)));
	}

	let report = Report {
		sender: sender_user.to_owned(),
		room_id: Some(body.room_id.clone()),
		event_id: None,
		user_id: None,
		report_type: "room".to_owned(),
		reason: body.reason.clone(),
	};

	services.admin.send_message(build_report(report)).await.ok();

	Ok(report_room::v3::Response {})
}

/// # `POST /_matrix/client/v3/rooms/{roomId}/report/{eventId}`
///
/// Reports an inappropriate event to homeserver admins
#[tracing::instrument(skip_all, fields(%client), name = "report_event", level = "info")]
pub(crate) async fn report_event_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<report_content::v3::Request>,
) -> Result<report_content::v3::Response> {
	// user authentication
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	delay_response().await;

	// check if we know about the reported event ID or if it's invalid
	let Ok(pdu) = services.rooms.timeline.get_pdu(&body.event_id).await else {
		return Err!(Request(NotFound("Event ID is not known to us or Event ID is invalid")));
	};

	is_event_report_valid(
		&services,
		&pdu.event_id,
		&body.room_id,
		sender_user,
		body.reason.as_ref(),
		&pdu,
	)
	.await?;
	info!(
		"Received event report by user {sender_user} for room {} and event ID {}, with reason: \
		 \"{}\"",
		body.room_id,
		body.event_id,
		body.reason.as_deref().unwrap_or("")
	);
	let report = Report {
		sender: sender_user.to_owned(),
		room_id: Some(body.room_id.clone()),
		event_id: Some(body.event_id.clone()),
		user_id: None,
		report_type: "event".to_owned(),
		reason: body.reason.clone(),
	};
	services.admin.send_message(build_report(report)).await.ok();

	Ok(report_content::v3::Response {})
}

#[tracing::instrument(skip_all, fields(%client), name = "report_user", level = "info")]
pub(crate) async fn report_user_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<report_user::v3::Request>,
) -> Result<report_user::v3::Response> {
	// user authentication
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	if body.reason.as_ref().is_some_and(|s| s.len() > 750) {
		return Err!(Request(
			InvalidParam("Reason too long, should be 750 characters or fewer",)
		));
	}

	delay_response().await;

	if !services.users.is_active_local(&body.user_id).await {
		// return 200 as to not reveal if the user exists. Recommended by spec.
		return Ok(report_user::v3::Response {});
	}

	let report = Report {
		sender: sender_user.to_owned(),
		room_id: None,
		event_id: None,
		user_id: Some(body.user_id.clone()),
		report_type: "user".to_owned(),
		reason: body.reason.clone(),
	};

	info!(
		"Received room report from {sender_user} for user {} with reason: \"{}\"",
		body.user_id,
		body.reason.as_deref().unwrap_or("")
	);

	services.admin.send_message(build_report(report)).await.ok();

	Ok(report_user::v3::Response {})
}

/// in the following order:
///
/// check if the room ID from the URI matches the PDU's room ID
/// check if score is in valid range
/// check if report reasoning is less than or equal to 750 characters
/// check if reporting user is in the reporting room
async fn is_event_report_valid(
	services: &Services,
	event_id: &EventId,
	room_id: &RoomId,
	sender_user: &UserId,
	reason: Option<&String>,
	pdu: &PduEvent,
) -> Result<()> {
	debug_info!(
		"Checking if report from user {sender_user} for event {event_id} in room {room_id} is \
		 valid"
	);

	if room_id != pdu.room_id_or_hash() {
		return Err!(Request(NotFound("Event ID does not belong to the reported room",)));
	}

	if reason.as_ref().is_some_and(|s| s.len() > 750) {
		return Err!(Request(
			InvalidParam("Reason too long, should be 750 characters or fewer",)
		));
	}

	if !services
		.rooms
		.state_cache
		.room_members(room_id)
		.ready_any(|user_id| user_id == sender_user)
		.await
	{
		return Err!(Request(NotFound("You are not in the room you are reporting.",)));
	}

	Ok(())
}

/// Builds a report message to be sent to the admin room.
fn build_report(report: Report) -> RoomMessageEventContent {
	let mut text =
		format!("@room New {} report received from {}:\n\n", report.report_type, report.sender);
	if report.user_id.is_some() {
		let _ = writeln!(text, "- Reported User ID: `{}`", report.user_id.unwrap());
	}
	if report.room_id.is_some() {
		let _ = writeln!(text, "- Reported Room ID: `{}`", report.room_id.unwrap());
	}
	if report.event_id.is_some() {
		let _ = writeln!(text, "- Reported Event ID: `{}`", report.event_id.unwrap());
	}
	if let Some(reason) = report.reason {
		let _ = writeln!(text, "- Report Reason: {reason}");
	}

	RoomMessageEventContent::text_markdown(text).add_mentions(Mentions::with_room_mention())
}

/// even though this is kinda security by obscurity, let's still make a small
/// random delay sending a response per spec suggestion regarding
/// enumerating for potential events existing in our server.
async fn delay_response() {
	let time_to_wait = rand::random_range(2..5);
	debug_info!(
		"Got successful /report request, waiting {time_to_wait} seconds before sending \
		 successful response."
	);

	sleep(Duration::from_secs(time_to_wait)).await;
}
