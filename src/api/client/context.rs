use axum::extract::State;
use conduwuit::{
	Err, Event, Result, at, debug_warn, err, ref_at,
	utils::{
		IterStream,
		future::TryExtExt,
		stream::{BroadbandExt, ReadyExt, TryIgnore, WidebandExt},
	},
};
use conduwuit_service::rooms::{lazy_loading, lazy_loading::Options, short::ShortStateKey};
use futures::{
	FutureExt, StreamExt, TryFutureExt, TryStreamExt,
	future::{OptionFuture, join, join3, try_join3},
};
use ruma::{OwnedEventId, UserId, api::client::context::get_context, events::StateEventType};

use crate::{
	Ruma,
	client::message::{event_filter, ignored_filter, lazy_loading_witness, visibility_filter},
};

const LIMIT_MAX: usize = 100;
const LIMIT_DEFAULT: usize = 10;

/// # `GET /_matrix/client/r0/rooms/{roomId}/context/{eventId}`
///
/// Allows loading room history around an event.
///
/// - Only works if the user is joined (TODO: always allow, but only show events
///   if the user was joined, depending on history_visibility)
pub(crate) async fn get_context_route(
	State(services): State<crate::State>,
	body: Ruma<get_context::v3::Request>,
) -> Result<get_context::v3::Response> {
	let sender = body.sender();
	let (sender_user, sender_device) = sender;
	let room_id = &body.room_id;
	let event_id = &body.event_id;
	let filter = &body.filter;

	if !services.rooms.metadata.exists(room_id).await {
		return Err!(Request(Forbidden("Room does not exist to this server")));
	}

	// Use limit or else 10, with maximum 100
	let limit: usize = body
		.limit
		.try_into()
		.unwrap_or(LIMIT_DEFAULT)
		.min(LIMIT_MAX);

	let base_id = services
		.rooms
		.timeline
		.get_pdu_id(event_id)
		.map_err(|_| err!(Request(NotFound("Event not found."))));

	let base_pdu = services
		.rooms
		.timeline
		.get_pdu(event_id)
		.map_err(|_| err!(Request(NotFound("Event not found."))));

	let visible = services
		.rooms
		.state_accessor
		.user_can_see_event(sender_user, room_id, event_id)
		.map(Ok);

	let (base_id, base_pdu, visible) = try_join3(base_id, base_pdu, visible).await?;

	if base_pdu.room_id_or_hash() != *room_id || base_pdu.event_id != *event_id {
		return Err!(Request(NotFound("Event not found.")));
	}

	if !visible {
		debug_warn!(req_evt = %event_id, ?base_id, %room_id, "Event requested by {sender_user} but is not allowed to see it, returning 404");
		return Err!(Request(NotFound("Event not found.")));
	}

	let base_count = base_id.pdu_count();

	let base_event = ignored_filter(&services, (base_count, base_pdu), sender_user);

	// PDUs are used to get seen user IDs and then returned in response.

	let events_before = services
		.rooms
		.timeline
		.pdus_rev(room_id, Some(base_count))
		.ignore_err()
		.then(async |mut pdu| {
			pdu.1.set_unsigned(Some(sender_user));
			if let Err(e) = services
				.rooms
				.pdu_metadata
				.add_bundled_aggregations_to_pdu(sender_user, &mut pdu.1)
				.await
			{
				debug_warn!("Failed to add bundled aggregations: {e}");
			}
			pdu
		})
		.ready_filter_map(|item| event_filter(item, filter))
		.wide_filter_map(|item| ignored_filter(&services, item, sender_user))
		.wide_filter_map(|item| visibility_filter(&services, item, sender_user))
		.take(limit / 2)
		.collect();

	let events_after = services
		.rooms
		.timeline
		.pdus(room_id, Some(base_count))
		.ignore_err()
		.then(async |mut pdu| {
			pdu.1.set_unsigned(Some(sender_user));
			if let Err(e) = services
				.rooms
				.pdu_metadata
				.add_bundled_aggregations_to_pdu(sender_user, &mut pdu.1)
				.await
			{
				debug_warn!("Failed to add bundled aggregations: {e}");
			}
			pdu
		})
		.ready_filter_map(|item| event_filter(item, filter))
		.wide_filter_map(|item| ignored_filter(&services, item, sender_user))
		.wide_filter_map(|item| visibility_filter(&services, item, sender_user))
		.take(limit / 2)
		.collect();

	let (base_event, events_before, events_after): (_, Vec<_>, Vec<_>) =
		join3(base_event, events_before, events_after).boxed().await;

	let lazy_loading_context = lazy_loading::Context {
		user_id: sender_user,
		device_id: Some(sender_device),
		room_id,
		token: Some(base_count.into_unsigned()),
		options: Some(&filter.lazy_load_options),
	};

	let lazy_loading_witnessed: OptionFuture<_> = filter
		.lazy_load_options
		.is_enabled()
		.then_some(
			base_event
				.iter()
				.chain(events_before.iter())
				.chain(events_after.iter()),
		)
		.map(|witnessed| lazy_loading_witness(&services, &lazy_loading_context, witnessed))
		.into();

	let state_at = events_after
		.last()
		.map(ref_at!(1))
		.map_or_else(|| body.event_id.as_ref(), |pdu| pdu.event_id.as_ref());

	let state_ids = services
		.rooms
		.state_accessor
		.pdu_shortstatehash(state_at)
		.or_else(|_| services.rooms.state.get_room_shortstatehash(room_id))
		.map_ok(|shortstatehash| {
			services
				.rooms
				.state_accessor
				.state_full_ids(shortstatehash)
				.map(Ok)
		})
		.map_err(|e| err!(Database("State not found: {e}")))
		.try_flatten_stream()
		.try_collect()
		.boxed();

	let (lazy_loading_witnessed, state_ids) = join(lazy_loading_witnessed, state_ids).await;

	let state_ids: Vec<(ShortStateKey, OwnedEventId)> = state_ids?;
	let shortstatekeys = state_ids.iter().map(at!(0)).stream();
	let shorteventids = state_ids.iter().map(ref_at!(1)).stream();
	let lazy_loading_witnessed = lazy_loading_witnessed.unwrap_or_default();
	let state: Vec<_> = services
		.rooms
		.short
		.multi_get_statekey_from_short(shortstatekeys)
		.zip(shorteventids)
		.ready_filter_map(|item| Some((item.0.ok()?, item.1)))
		.ready_filter_map(|((event_type, state_key), event_id)| {
			if filter.lazy_load_options.is_enabled()
				&& event_type == StateEventType::RoomMember
				&& state_key
					.as_str()
					.try_into()
					.is_ok_and(|user_id: &UserId| !lazy_loading_witnessed.contains(user_id))
			{
				return None;
			}

			Some(event_id)
		})
		.broad_filter_map(|event_id: &OwnedEventId| {
			services.rooms.timeline.get_pdu(event_id.as_ref()).ok()
		})
		.map(Event::into_format)
		.collect()
		.await;

	Ok(get_context::v3::Response {
		event: base_event.map(at!(1)).map(Event::into_format),

		start: events_before
			.last()
			.map(at!(0))
			.or(Some(base_count))
			.as_ref()
			.map(ToString::to_string),

		end: events_after
			.last()
			.map(at!(0))
			.or(Some(base_count))
			.as_ref()
			.map(ToString::to_string),

		events_before: events_before
			.into_iter()
			.map(at!(1))
			.map(Event::into_format)
			.collect(),

		events_after: events_after
			.into_iter()
			.map(at!(1))
			.map(Event::into_format)
			.collect(),

		state,
	})
}
