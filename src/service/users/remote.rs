use conduwuit::utils::stream::TryIgnore;
use futures::Stream;
use ruma::{OwnedUserId, UserId};

impl super::Service {
	/// Record the existence of a remote user.
	pub fn record_remote_user(&self, user_id: &UserId) {
		assert!(self.services.globals.user_is_local(user_id), "user is not remote");

		self.db.remoteuserid_remoteuser.insert(user_id, "");
	}

	/// Returns a stream over all remote users this server has ever seen.
	pub fn stream_remote_users(&self) -> impl Stream<Item = OwnedUserId> + Send {
		self.db.remoteuserid_remoteuser.keys().ignore_err()
	}
}
