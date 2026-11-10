use std::{collections::BTreeMap, sync::Arc};

use conduwuit::result::LogErr;
use conduwuit_core::{
	Event, Result, err,
	matrix::{
		event::RelationTypeEqual,
		pdu::{PduCount, PduEvent, PduId, RawPduId},
	},
	utils::{
		ReadyExt,
		stream::{TryIgnore, WidebandExt},
	},
};
use conduwuit_database::{Deserialized, Map};
use futures::{Stream, StreamExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedUserId, RoomId, UInt, UserId,
	api::{Direction, client::threads::get_threads::v1::IncludeThreads},
	events::{
		AnySyncMessageLikeEvent, AnySyncTimelineEvent,
		relation::{BundledThread, RelationType},
	},
	serde::Raw,
	uint,
};
use serde_json::json;

use crate::{Dep, rooms, rooms::short::ShortRoomId};

pub struct Service {
	db: Data,
	services: Services,
}

struct Services {
	short: Dep<rooms::short::Service>,
	pdu_metadata: Dep<rooms::pdu_metadata::Service>,
	timeline: Dep<rooms::timeline::Service>,
}

pub(super) struct Data {
	threadid_userids: Arc<Map>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				threadid_userids: args.db["threadid_userids"].clone(),
			},
			services: Services {
				short: args.depend::<rooms::short::Service>("rooms::short"),
				pdu_metadata: args.depend::<rooms::pdu_metadata::Service>("rooms::pdu_metadata"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Serializes an event for the `latest_event` of a thread's aggregation,
	/// stripping `unsigned` as we don't want to send e.g. txnid to clients.
	fn latest_event<E>(event: &E) -> Raw<AnySyncMessageLikeEvent>
	where
		E: Event,
	{
		let mut latest_pdu = event.clone().into_pdu();
		latest_pdu.unsigned = None;
		latest_pdu
			.into_format::<Raw<AnySyncTimelineEvent>>()
			.cast_unchecked()
	}

	fn set_thread(unsigned: &mut CanonicalJsonObject, thread: BundledThread) {
		let content = serde_json::to_value(thread).expect("to_value always works");
		unsigned.insert(
			"m.relations".to_owned(),
			json!({ "m.thread": content })
				.try_into()
				.expect("thread is valid json"),
		);
	}

	/// Remove `event` from the aggregation of the thread matching
	/// `root_event_id`
	pub async fn remove_from_thread<E>(&self, root_event_id: &EventId, event: &E) -> Result
	where
		E: Event + Send + Sync,
	{
		let Ok(root_id) = self.services.timeline.get_pdu_id(root_event_id).await else {
			return Ok(());
		};

		let mut root_pdu_json = self
			.services
			.timeline
			.get_pdu_json_from_id(&root_id)
			.await
			.map_err(|e| err!(Database("Thread root pdu not found: {e:?}")))?;

		let Some(CanonicalJsonValue::Object(unsigned)) = root_pdu_json.get_mut("unsigned") else {
			return Ok(());
		};

		let thread = self
			.services
			.pdu_metadata
			.get_relations(
				event.sender(),
				&event.room_id_or_hash(),
				root_event_id,
				PduCount::max(),
				usize::MAX,
				0,
				Direction::Backward,
				|pdu| RelationType::Thread.relation_type_equal(pdu),
			)
			.await;

		match thread.first() {
			| Some((_, latest)) => Self::set_thread(
				unsigned,
				BundledThread::new(
					Self::latest_event(latest),
					thread.len().try_into().unwrap_or(UInt::MAX),
					true,
				),
			),
			| None => {
				// No events are left in the thread, or the root is backfilled
				unsigned.remove("m.relations");
			},
		}

		self.services
			.timeline
			.replace_pdu(&root_id, &root_pdu_json)
			.await
	}

	pub async fn add_to_thread<E>(&self, root_event_id: &EventId, event: &E) -> Result
	where
		E: Event + Send + Sync,
	{
		let root_id = self
			.services
			.timeline
			.get_pdu_id(root_event_id)
			.await
			.map_err(|e| {
				err!(Request(InvalidParam("Invalid event_id in thread message: {e:?}")))
			})?;

		let root_pdu = self
			.services
			.timeline
			.get_pdu_from_id(&root_id)
			.await
			.map_err(|e| err!(Request(InvalidParam("Thread root not found: {e:?}"))))?;

		let mut root_pdu_json = self
			.services
			.timeline
			.get_pdu_json_from_id(&root_id)
			.await
			.map_err(|e| err!(Request(InvalidParam("Thread root pdu not found: {e:?}"))))?;

		// get_relations doesn't support backfilled targets, so redactions
		// couldn't update the aggregation of a backfilled root
		//
		// TODO: Support backfilled threads. This needs
		// `src/service/rooms/pdu_metadata/mod.rs`'s
		// (TODO: Relations with backfilled pdus) to be implemented,
		// and then this check can be removed.
		if matches!(root_id.pdu_count(), PduCount::Normal(_))
			&& let CanonicalJsonValue::Object(unsigned) = root_pdu_json
				.entry("unsigned".to_owned())
				.or_insert_with(|| CanonicalJsonValue::Object(BTreeMap::default()))
		{
			let latest_event = Self::latest_event(event);
			let thread = match unsigned
				.get("m.relations")
				.and_then(|r| r.as_object())
				.and_then(|r| r.get("m.thread"))
				.and_then(|relations| {
					serde_json::from_value::<BundledThread>(relations.clone().into()).ok()
				}) {
				// Thread already existed
				| Some(mut thread) => {
					thread.count = thread.count.saturating_add(uint!(1));
					thread.latest_event = latest_event;
					thread
				},
				// New thread
				| None => BundledThread::new(latest_event, uint!(1), true),
			};

			Self::set_thread(unsigned, thread);

			self.services
				.timeline
				.replace_pdu(&root_id, &root_pdu_json)
				.await?;
		}

		let mut users = Vec::new();
		match self.get_participants(&root_id).await {
			| Ok(userids) => {
				users.extend_from_slice(&userids);
			},
			| _ => {
				users.push(root_pdu.sender().to_owned());
			},
		}
		users.push(event.sender().to_owned());

		self.update_participants(&root_id, &users)
	}

	pub async fn threads_until<'a>(
		&'a self,
		user_id: &'a UserId,
		room_id: &'a RoomId,
		shorteventid: PduCount,
		_inc: &'a IncludeThreads,
	) -> Result<impl Stream<Item = (PduCount, PduEvent)> + Send + 'a> {
		let shortroomid: ShortRoomId = self.services.short.get_shortroomid(room_id).await?;

		let current: RawPduId = PduId {
			shortroomid,
			shorteventid: shorteventid.saturating_sub(1),
		}
		.into();

		let stream = self
			.db
			.threadid_userids
			.rev_raw_keys_from(&current)
			.ignore_err()
			.map(RawPduId::from)
			.ready_take_while(move |pdu_id| pdu_id.shortroomid() == shortroomid.to_be_bytes())
			.wide_filter_map(move |pdu_id| async move {
				let mut pdu = self.services.timeline.get_pdu_from_id(&pdu_id).await.ok()?;

				let pdu_id: PduId = pdu_id.into();
				let ctx = self
					.services
					.timeline
					.get_unsigned_context(&pdu, Some(user_id))
					.await;
				pdu.set_unsigned(Some(ctx)).log_err().ok();

				Some((pdu_id.shorteventid, pdu))
			});

		Ok(stream)
	}

	pub(super) fn update_participants(
		&self,
		root_id: &RawPduId,
		participants: &[OwnedUserId],
	) -> Result {
		let users = participants
			.iter()
			.map(|user| user.as_bytes())
			.collect::<Vec<_>>()
			.join(&[0xFF][..]);

		self.db.threadid_userids.insert(root_id, &users);

		Ok(())
	}

	pub(super) async fn get_participants(&self, root_id: &RawPduId) -> Result<Vec<OwnedUserId>> {
		self.db.threadid_userids.get(root_id).await.deserialized()
	}
}
