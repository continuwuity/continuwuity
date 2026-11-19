use std::sync::Arc;

use conduwuit::{
	Err, PduCount, PduEvent, Result, at, err,
	result::NotFound,
	utils::{
		self,
		stream::{ReadyExt, TryReadyExt},
	},
};
use database::{Database, Deserialized, Json, KeyVal, Map};
use futures::{
	FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt, future::select_ok, pin_mut,
};
use ruma::{
	CanonicalJsonObject, EventId, MilliSecondsSinceUnixEpoch, OwnedUserId, RoomId, api::Direction,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue as RawJsonValue;

use super::{PduId, RawPduId};
use crate::{Dep, rooms, rooms::short::ShortRoomId};

pub(super) struct Data {
	eventid_outlierpdu: Arc<Map>,
	eventid_pduid: Arc<Map>,
	pduid_pdu: Arc<Map>,
	pduid_unredactedcontent: Arc<Map>,
	roomid_lasttimelinecount: Arc<Map>,
	roomts_pduid: Arc<Map>,
	userroomid_highlightcount: Arc<Map>,
	userroomid_notificationcount: Arc<Map>,
	pub(super) db: Arc<Database>,
	services: Services,
}

struct Services {
	short: Dep<rooms::short::Service>,
}

pub type PdusIterItem = (PduCount, PduEvent);

#[derive(Deserialize, Serialize)]
pub(super) struct UnredactedContent {
	pub(super) created: u64,
	pub(super) content: Box<RawJsonValue>,
}

impl Data {
	pub(super) fn new(args: &crate::Args<'_>) -> Self {
		let db = &args.db;
		Self {
			eventid_outlierpdu: db["eventid_outlierpdu"].clone(),
			eventid_pduid: db["eventid_pduid"].clone(),
			pduid_pdu: db["pduid_pdu"].clone(),
			pduid_unredactedcontent: db["pduid_unredactedcontent"].clone(),
			roomid_lasttimelinecount: db["roomid_lasttimelinecount"].clone(),
			roomts_pduid: db["roomts_pduid"].clone(),
			userroomid_highlightcount: db["userroomid_highlightcount"].clone(),
			userroomid_notificationcount: db["userroomid_notificationcount"].clone(),
			db: args.db.clone(),
			services: Services {
				short: args.depend::<rooms::short::Service>("rooms::short"),
			},
		}
	}

	#[inline]
	pub(super) async fn last_timeline_count(&self, room_id: &RoomId) -> Result<PduCount> {
		let pdus_rev = self.pdus_rev(room_id, PduCount::max());

		pin_mut!(pdus_rev);
		let last_count = pdus_rev
			.try_next()
			.await?
			.map(at!(0))
			.filter(|&count| matches!(count, PduCount::Normal(_)))
			.unwrap_or_else(PduCount::max);

		Ok(last_count)
	}

	pub(super) async fn room_activity(&self, room_id: &RoomId) -> Result<u64> {
		if let Ok(count) = self
			.roomid_lasttimelinecount
			.get(room_id)
			.await
			.deserialized()
		{
			return Ok(count);
		}

		let pdus_rev = self.pdus_rev(room_id, PduCount::max());
		pin_mut!(pdus_rev);
		let Some((PduCount::Normal(count), _)) = pdus_rev.try_next().await? else {
			return Ok(0);
		};

		self.roomid_lasttimelinecount
			.put_raw(room_id.as_bytes(), count.to_be_bytes());
		Ok(count)
	}

	#[inline]
	pub(super) async fn latest_pdu_in_room(&self, room_id: &RoomId) -> Result<PduEvent> {
		let pdus_rev = self.pdus_rev(room_id, PduCount::max());

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

	pub(super) async fn append_pdu(
		&self,
		pdu_id: &RawPduId,
		pdu: &PduEvent,
		room_id: &RoomId,
		json: &CanonicalJsonObject,
		count: PduCount,
		index_ts: MilliSecondsSinceUnixEpoch,
	) {
		debug_assert!(matches!(count, PduCount::Normal(_)), "PduCount not Normal");

		self.pduid_pdu.raw_put(pdu_id, Json(json));
		self.roomts_pduid.insert(&roomts_key(pdu_id, index_ts), []);
		self.eventid_pduid.insert(pdu.event_id.as_bytes(), pdu_id);
		self.eventid_outlierpdu.remove(pdu.event_id.as_bytes());
		self.roomid_lasttimelinecount
			.put_raw(room_id.as_bytes(), count.into_unsigned().to_be_bytes());
	}

	pub(super) fn prepend_backfill_pdu(
		&self,
		pdu_id: &RawPduId,
		event_id: &EventId,
		json: &CanonicalJsonObject,
		index_ts: MilliSecondsSinceUnixEpoch,
	) {
		self.pduid_pdu.raw_put(pdu_id, Json(json));
		self.roomts_pduid.insert(&roomts_key(pdu_id, index_ts), []);
		self.eventid_pduid.insert(event_id, pdu_id);
		self.eventid_outlierpdu.remove(event_id);
	}

	/// Removes a pdu and creates a new one with the same id.
	pub(super) async fn replace_pdu(
		&self,
		pdu_id: &RawPduId,
		pdu_json: &CanonicalJsonObject,
	) -> Result {
		if self.pduid_pdu.get(pdu_id).await.is_not_found() {
			return Err!(Request(NotFound("PDU does not exist.")));
		}

		self.pduid_pdu.raw_put(pdu_id, Json(pdu_json));

		Ok(())
	}

	pub(super) fn save_unredacted_content(&self, pdu_id: &RawPduId, content: &RawJsonValue) {
		let entry = UnredactedContent {
			created: utils::millis_since_unix_epoch(),
			content: content.to_owned(),
		};

		self.pduid_unredactedcontent.raw_put(pdu_id, Json(&entry));
	}

	pub(super) async fn get_unredacted_content(
		&self,
		pdu_id: &RawPduId,
	) -> Result<UnredactedContent> {
		self.pduid_unredactedcontent
			.get(pdu_id)
			.await
			.deserialized()
	}

	pub(super) async fn purge_unredacted_content(&self, cutoff: u64) -> usize {
		let expired: Vec<_> = self
			.pduid_unredactedcontent
			.raw_stream()
			.ready_filter_map(Result::ok)
			.ready_filter_map(|(key, val)| {
				let entry: UnredactedContent = serde_json::from_slice(val).ok()?;
				(entry.created < cutoff).then(|| key.to_vec())
			})
			.collect()
			.await;

		for key in &expired {
			self.pduid_unredactedcontent.remove(key);
		}

		expired.len()
	}

	/// Returns an iterator over all events and their tokens in a room that
	/// happened before the event with id `until` in reverse-chronological
	/// order.
	pub(super) fn pdus_rev<'a>(
		&'a self,
		room_id: &'a RoomId,
		until: PduCount,
	) -> impl Stream<Item = Result<PdusIterItem>> + Send + 'a {
		self.count_to_id(room_id, until, Direction::Backward)
			.map_ok(move |current| {
				let prefix = current.shortroomid();
				self.pduid_pdu
					.rev_raw_stream_from(&current)
					.ready_try_take_while(move |(key, _)| Ok(key.starts_with(&prefix)))
					.ready_and_then(Self::from_json_slice)
			})
			.try_flatten_stream()
	}

	pub(super) fn pdus<'a>(
		&'a self,
		room_id: &'a RoomId,
		from: PduCount,
	) -> impl Stream<Item = Result<PdusIterItem>> + Send + 'a {
		self.count_to_id(room_id, from, Direction::Forward)
			.map_ok(move |current| {
				let prefix = current.shortroomid();
				self.pduid_pdu
					.raw_stream_from(&current)
					.ready_try_take_while(move |(key, _)| Ok(key.starts_with(&prefix)))
					.ready_and_then(Self::from_json_slice)
			})
			.try_flatten_stream()
	}

	pub(super) fn pdus_by_timestamp<'a>(
		&'a self,
		room_id: &'a RoomId,
		timestamp: MilliSecondsSinceUnixEpoch,
		dir: Direction,
	) -> impl Stream<Item = Result<PdusIterItem>> + Send + 'a {
		self.services
			.short
			.get_shortroomid(room_id)
			.map_err(move |e| err!(Request(NotFound("Room {room_id:?} not found: {e:?}"))))
			.map_ok(move |shortroomid| {
				let seek = |shorteventid| {
					roomts_key(&PduId { shortroomid, shorteventid }.into(), timestamp)
				};
				let keys = match dir {
					| Direction::Forward => self
						.roomts_pduid
						.raw_keys_from(&seek(PduCount::min()))
						.left_stream(),
					| Direction::Backward => self
						.roomts_pduid
						.rev_raw_keys_from(&seek(PduCount::max()))
						.right_stream(),
				};

				keys.ready_try_take_while(move |key| {
					Ok(key.starts_with(&shortroomid.to_be_bytes()))
				})
				.map_ok(roomts_key_to_pdu_id)
				.and_then(async move |pdu_id| {
					Ok((pdu_id.pdu_count(), self.get_pdu_from_id(&pdu_id).await?))
				})
			})
			.try_flatten_stream()
	}

	pub(super) async fn timestamp_index_ready(&self) -> bool {
		self.db["global"]
			.exists(TIMESTAMP_INDEX_MIGRATION)
			.await
			.is_ok()
	}

	fn from_json_slice((pdu_id, pdu): KeyVal<'_>) -> Result<PdusIterItem> {
		let pdu_id: RawPduId = pdu_id.into();

		let pdu = serde_json::from_slice::<PduEvent>(pdu)?;

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

pub const TIMESTAMP_INDEX_MIGRATION: &str = "populate_roomts_pduid";

#[must_use]
pub fn roomts_key(pdu_id: &RawPduId, ts: MilliSecondsSinceUnixEpoch) -> Vec<u8> {
	let count = pdu_id.pdu_count().into_signed().cast_unsigned() ^ (1 << 63);
	[pdu_id.shortroomid(), u64::from(ts.get()).to_be_bytes(), count.to_be_bytes()].concat()
}

fn roomts_key_to_pdu_id(key: &[u8]) -> RawPduId {
	let shortroomid = u64::from_be_bytes(key[0..8].try_into().expect("roomts key shortroomid"));
	let count = u64::from_be_bytes(key[16..24].try_into().expect("roomts key count")) ^ (1 << 63);

	PduId {
		shortroomid,
		shorteventid: PduCount::from_signed(count.cast_signed()),
	}
	.into()
}

//TODO: this is an ABA
fn increment(db: &Arc<Map>, key: &[u8]) {
	let old = db.get_blocking(key);
	let new = utils::increment(old.ok().as_deref());
	db.insert(key, new);
}

#[cfg(test)]
mod tests {
	use ruma::uint;

	use super::*;

	#[test]
	fn roomts_key_orders_by_ts_then_count() {
		let ids = [
			(PduCount::Normal(9), uint!(1)),
			(PduCount::Backfilled(-5), uint!(2)),
			(PduCount::Normal(3), uint!(2)),
			(PduCount::Normal(1), uint!(3)),
		]
		.map(|(count, ts)| (RawPduId::from(PduId { shortroomid: 7, shorteventid: count }), ts));
		let keys = ids.map(|(id, ts)| roomts_key(&id, MilliSecondsSinceUnixEpoch(ts)));

		assert!(keys.is_sorted());
		for ((id, _), key) in ids.iter().zip(&keys) {
			assert_eq!(roomts_key_to_pdu_id(key), *id);
		}
	}
}
