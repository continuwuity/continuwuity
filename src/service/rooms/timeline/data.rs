use std::{borrow::Borrow, sync::Arc};

use conduwuit::{
	Err, PduCount, PduEvent, Result, at, err,
	result::{LogErr, NotFound},
	utils,
	utils::stream::TryReadyExt,
};
use database::{Database, Deserialized, Json, KeyVal, Map};
use futures::{FutureExt, Stream, TryFutureExt, TryStreamExt, future::select_ok, pin_mut};
use ruma::{CanonicalJsonObject, EventId, OwnedUserId, RoomId, UserId, api::Direction};

use super::{PduId, RawPduId};
use crate::{Dep, rooms, rooms::short::ShortRoomId};

pub(super) struct Data {
	eventid_outlierpdu: Arc<Map>,
	eventid_pduid: Arc<Map>,
	pduid_pdu: Arc<Map>,
	userroomid_highlightcount: Arc<Map>,
	userroomid_notificationcount: Arc<Map>,
	/// Stores the original content of redacted PDUs.
	pduid_originalcontent: Arc<Map>,
	pub(super) db: Arc<Database>,
	services: Services,
}

struct Services {
	short: Dep<rooms::short::Service>,
}

pub type PdusIterItem = (PduCount, PduEvent);

impl Data {
	pub(super) fn new(args: &crate::Args<'_>) -> Self {
		let db = &args.db;
		Self {
			eventid_outlierpdu: db["eventid_outlierpdu"].clone(),
			eventid_pduid: db["eventid_pduid"].clone(),
			pduid_pdu: db["pduid_pdu"].clone(),
			userroomid_highlightcount: db["userroomid_highlightcount"].clone(),
			userroomid_notificationcount: db["userroomid_notificationcount"].clone(),
			pduid_originalcontent: db["pduid_originalcontent"].clone(), // Initialize new table
			db: args.db.clone(),
			services: Services {
				short: args.depend::<rooms::short::Service>("rooms::short"),
			},
		}
	}

	#[inline]
	pub(super) async fn last_timeline_count(
		&self,
		sender_user: Option<&UserId>,
		room_id: &RoomId,
	) -> Result<PduCount> {
		let pdus_rev = self.pdus_rev(sender_user, room_id, PduCount::max());

		pin_mut!(pdus_rev);
		let last_count = pdus_rev
			.try_next()
			.await?
			.map(at!(0))
			.filter(|&count| matches!(count, PduCount::Normal(_)))
			.unwrap_or_else(PduCount::max);

		Ok(last_count)
	}

	#[inline]
	pub(super) async fn latest_pdu_in_room(
		&self,
		sender_user: Option<&UserId>,
		room_id: &RoomId,
	) -> Result<PduEvent> {
		let pdus_rev = self.pdus_rev(sender_user, room_id, PduCount::max());

		pin_mut!(pdus_rev);
		pdus_rev
			.try_next()
			.await?
			.map(at!(1))
			.ok_or_else(|| err!(Request(NotFound("no PDU's found in room"))))
	}

	/// Returns the `count` of this pdu's id.
	pub(super) async fn get_pdu_count(&self, event_id: &EventId) -> Result<PduCount> {
		self.get_pdu_id(event_id)
			.await
			.map(|pdu_id| pdu_id.pdu_count())
	}

	/// Returns the json of a pdu.
	pub(super) async fn get_pdu_json(&self, event_id: &EventId) -> Result<CanonicalJsonObject> {
		let accepted = self.get_non_outlier_pdu_json(event_id).boxed();
		let outlier = self
			.eventid_outlierpdu
			.get(event_id)
			.map(Deserialized::deserialized)
			.boxed();

		select_ok([accepted, outlier]).await.map(at!(0))
	}

	/// Returns the json of a pdu.
	pub(super) async fn get_non_outlier_pdu_json(
		&self,
		event_id: &EventId,
	) -> Result<CanonicalJsonObject> {
		let pduid = self.get_pdu_id(event_id).await?;

		self.pduid_pdu.get(&pduid).await.deserialized()
	}

	/// Returns the pdu's id.
	#[inline]
	pub(super) async fn get_pdu_id(&self, event_id: &EventId) -> Result<RawPduId> {
		self.eventid_pduid
			.get(event_id)
			.await
			.map(|handle| RawPduId::from(&*handle))
	}

	/// Returns the pdu directly from `eventid_pduid` only.
	pub(super) async fn get_non_outlier_pdu(&self, event_id: &EventId) -> Result<PduEvent> {
		let pduid = self.get_pdu_id(event_id).await?;

		self.pduid_pdu.get(&pduid).await.deserialized()
	}

	/// Like get_non_outlier_pdu(), but without the expense of fetching and
	/// parsing the PduEvent
	pub(super) async fn non_outlier_pdu_exists(&self, event_id: &EventId) -> Result {
		let pduid = self.get_pdu_id(event_id).await?;

		self.pduid_pdu.exists(&pduid).await
	}

	/// Returns the pdu.
	///
	/// Checks the `eventid_outlierpdu` Tree if not found in the timeline.
	pub(super) async fn get_pdu(&self, event_id: &EventId) -> Result<PduEvent> {
		let accepted = self.get_non_outlier_pdu(event_id).boxed();
		let outlier = self
			.eventid_outlierpdu
			.get(event_id)
			.map(Deserialized::deserialized)
			.boxed();

		select_ok([accepted, outlier]).await.map(at!(0))
	}

	/// Like get_non_outlier_pdu(), but without the expense of fetching and
	/// parsing the PduEvent
	#[inline]
	pub(super) async fn outlier_pdu_exists(&self, event_id: &EventId) -> Result {
		self.eventid_outlierpdu.exists(event_id).await
	}

	/// Like get_pdu(), but without the expense of fetching and parsing the data
	pub(super) async fn pdu_exists(&self, event_id: &EventId) -> Result {
		let non_outlier = self.non_outlier_pdu_exists(event_id).boxed();
		let outlier = self.outlier_pdu_exists(event_id).boxed();

		select_ok([non_outlier, outlier]).await.map(at!(0))
	}

	/// Returns the pdu.
	///
	/// This does __NOT__ check the outliers `Tree`.
	pub(super) async fn get_pdu_from_id(&self, pdu_id: &RawPduId) -> Result<PduEvent> {
		self.pduid_pdu.get(pdu_id).await.deserialized()
	}

	/// Returns the pdu as a `BTreeMap<String, CanonicalJsonValue>`.
	pub(super) async fn get_pdu_json_from_id(
		&self,
		pdu_id: &RawPduId,
	) -> Result<CanonicalJsonObject> {
		self.pduid_pdu.get(pdu_id).await.deserialized()
	}

	/// Stores the original content of a PDU that is about to be redacted.
	pub(super) async fn store_redacted_pdu_content(
		&self,
		pdu_id: &RawPduId,
		pdu_json: &CanonicalJsonObject,
	) -> Result<()> {
		self.pduid_originalcontent.raw_put(pdu_id, Json(pdu_json));
		Ok(())
	}

	pub(super) async fn append_pdu(
		&self,
		pdu_id: &RawPduId,
		pdu: &PduEvent,
		json: &CanonicalJsonObject,
		count: PduCount,
	) {
		debug_assert!(matches!(count, PduCount::Normal(_)), "PduCount not Normal");

		self.pduid_pdu.raw_put(pdu_id, Json(json));
		self.eventid_pduid.insert(pdu.event_id.as_bytes(), pdu_id);
		self.eventid_outlierpdu.remove(pdu.event_id.as_bytes());
	}

	pub(super) fn prepend_backfill_pdu(
		&self,
		pdu_id: &RawPduId,
		event_id: &EventId,
		json: &CanonicalJsonObject,
	) {
		self.pduid_pdu.raw_put(pdu_id, Json(json));
		self.eventid_pduid.insert(event_id, pdu_id);
		self.eventid_outlierpdu.remove(event_id);
	}

	/// Removes a pdu and creates a new one with the same id.
	pub(super) async fn replace_pdu(
		&self,
		pdu_id: &RawPduId,
		pdu_json: &CanonicalJsonObject,
		_pdu: &PduEvent,
	) -> Result {
		if self.pduid_pdu.get(pdu_id).await.is_not_found() {
			return Err!(Request(NotFound("PDU does not exist.")));
		}

		self.pduid_pdu.raw_put(pdu_id, Json(pdu_json));

		Ok(())
	}

	/// Returns an iterator over all events and their tokens in a room that
	/// happened before the event with id `until` in reverse-chronological
	/// order.
	pub(super) fn pdus_rev<'a>(
		&'a self,
		user_id: Option<&'a UserId>,
		room_id: &'a RoomId,
		until: PduCount,
	) -> impl Stream<Item = Result<PdusIterItem>> + Send + 'a {
		self.count_to_id(room_id, until, Direction::Backward)
			.map_ok(move |current| {
				let prefix = current.shortroomid();
				self.pduid_pdu
					.rev_raw_stream_from(&current)
					.ready_try_take_while(move |(key, _)| Ok(key.starts_with(&prefix)))
					.ready_and_then(move |item| Self::each_pdu(item, user_id))
			})
			.try_flatten_stream()
	}

	pub(super) fn pdus<'a>(
		&'a self,
		user_id: Option<&'a UserId>,
		room_id: &'a RoomId,
		from: PduCount,
	) -> impl Stream<Item = Result<PdusIterItem>> + Send + 'a {
		self.count_to_id(room_id, from, Direction::Forward)
			.map_ok(move |current| {
				let prefix = current.shortroomid();
				self.pduid_pdu
					.raw_stream_from(&current)
					.ready_try_take_while(move |(key, _)| Ok(key.starts_with(&prefix)))
					.ready_and_then(move |item| Self::each_pdu(item, user_id))
			})
			.try_flatten_stream()
	}

	fn each_pdu((pdu_id, pdu): KeyVal<'_>, user_id: Option<&UserId>) -> Result<PdusIterItem> {
		let pdu_id: RawPduId = pdu_id.into();

		let mut pdu = serde_json::from_slice::<PduEvent>(pdu)?;

		if Some(pdu.sender.borrow()) != user_id {
			pdu.remove_transaction_id().log_err().ok();
		}

		pdu.add_age().log_err().ok();

		Ok((pdu_id.pdu_count(), pdu))
	}

	pub(super) fn increment_notification_counts(
		&self,
		room_id: &RoomId,
		notifies: Vec<OwnedUserId>,
		highlights: Vec<OwnedUserId>,
	) {
		let _cork = self.db.cork();

		for user in notifies {
			let mut userroom_id = user.as_bytes().to_vec();
			userroom_id.push(0xFF);
			userroom_id.extend_from_slice(room_id.as_bytes());
			increment(&self.userroomid_notificationcount, &userroom_id);
		}

		for user in highlights {
			let mut userroom_id = user.as_bytes().to_vec();
			userroom_id.push(0xFF);
			userroom_id.extend_from_slice(room_id.as_bytes());
			increment(&self.userroomid_highlightcount, &userroom_id);
		}
	}

	async fn count_to_id(
		&self,
		room_id: &RoomId,
		shorteventid: PduCount,
		dir: Direction,
	) -> Result<RawPduId> {
		let shortroomid: ShortRoomId = self
			.services
			.short
			.get_shortroomid(room_id)
			.await
			.map_err(|e| err!(Request(NotFound("Room {room_id:?} not found: {e:?}"))))?;

		// +1 so we don't send the base event
		let pdu_id = PduId {
			shortroomid,
			shorteventid: shorteventid.saturating_inc(dir),
		};

		Ok(pdu_id.into())
	}
}

//TODO: this is an ABA
fn increment(db: &Arc<Map>, key: &[u8]) {
	let old = db.get_blocking(key);
	let new = utils::increment(old.ok().as_deref());
	db.insert(key, new);
}
