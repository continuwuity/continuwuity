use conduwuit::{Result, implement, trace};
use futures::{FutureExt, StreamExt, pin_mut, stream::FuturesUnordered};
use ruma::{DeviceId, UserId};

#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub async fn watch(&self, user_id: &UserId, device_id: &DeviceId) -> Result {
	let userid_bytes = user_id.as_bytes().to_vec();
	let mut userid_prefix = userid_bytes.clone();
	userid_prefix.push(0xFF);

	let mut userdeviceid_prefix = userid_prefix.clone();
	userdeviceid_prefix.extend_from_slice(device_id.as_bytes());
	userdeviceid_prefix.push(0xFF);

	let mut futures = FuturesUnordered::new();

	// Return when *any* user changed their key
	// TODO: only send for user they share a room with
	futures.push(self.db.todeviceid_events.watch_prefix(&userdeviceid_prefix));

	futures.push(self.db.userroomid_joined.watch_prefix(&userid_prefix));
	futures.push(self.db.userroomid_invitestate.watch_prefix(&userid_prefix));
	futures.push(self.db.userroomid_leftstate.watch_prefix(&userid_prefix));
	futures.push(
		self.db
			.userroomid_notificationcount
			.watch_prefix(&userid_prefix),
	);
	futures.push(
		self.db
			.userroomid_highlightcount
			.watch_prefix(&userid_prefix),
	);

	// Events for rooms we are in
	let rooms_joined = self.services.state_cache.rooms_joined(user_id);

	pin_mut!(rooms_joined);
	while let Some(room_id) = rooms_joined.next().await {
		let Ok(short_roomid) = self.services.short.get_shortroomid(&room_id).await else {
			continue;
		};

		let roomid_bytes = room_id.as_bytes().to_vec();
		let mut roomid_prefix = roomid_bytes.clone();
		roomid_prefix.push(0xFF);

		// Key changes
		futures.push(self.db.keychangeid_userid.watch_prefix(&roomid_prefix));

		// Room account data
		let mut roomuser_prefix = roomid_prefix.clone();
		roomuser_prefix.extend_from_slice(&userid_prefix);

		futures.push(
			self.db
				.roomusertype_roomuserdataid
				.watch_prefix(&roomuser_prefix),
		);

		// PDUs
		let short_roomid = short_roomid.to_be_bytes().to_vec();
		futures.push(self.db.pduid_pdu.watch_prefix(&short_roomid));

		// EDUs
		let typing_room_id = room_id.to_owned();
		let typing_wait_for_update = async move {
			self.services.typing.wait_for_update(&typing_room_id).await;
		};

		futures.push(typing_wait_for_update.boxed());
		futures.push(
			self.db
				.readreceiptid_readreceipt
				.watch_prefix(&roomid_prefix),
		);
	}

	let mut globaluserdata_prefix = vec![0xFF];
	globaluserdata_prefix.extend_from_slice(&userid_prefix);

	futures.push(
		self.db
			.roomusertype_roomuserdataid
			.watch_prefix(&globaluserdata_prefix),
	);

	// More key changes (used when user is not joined to any rooms)
	futures.push(self.db.keychangeid_userid.watch_prefix(&userid_prefix));

	// One time keys
	futures.push(
		self.db
			.userid_lastonetimekeyupdate
			.watch_prefix(&userid_bytes),
	);

	// Server shutdown
	futures.push(self.services.server.until_shutdown().boxed());

	if !self.services.server.running() {
		return Ok(());
	}

	// Wait until one of them finds something
	trace!(futures = futures.len(), "watch started");
	futures.next().await;
	trace!(futures = futures.len(), "watch finished");

	Ok(())
}
