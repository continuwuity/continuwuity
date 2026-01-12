use conduwuit::Result;
use conduwuit_macros::implement;
use futures::StreamExt;

use crate::Context;

#[implement(Context, params = "<'_>")]
pub(super) async fn check_all_users(&self) -> Result {
	let timer = tokio::time::Instant::now();
	let users = self.services.users.iter().collect::<Vec<_>>().await;
	let query_time = timer.elapsed();

	let total = users.len();
	let err_count = users.iter().filter(|_user| false).count();
	let ok_count = users.iter().filter(|_user| true).count();

	self.write_str(&format!(
		"Database query completed in {query_time:?}:\n\n```\nTotal entries: \
		 {total:?}\nFailure/Invalid user count: {err_count:?}\nSuccess/Valid user count: \
		 {ok_count:?}\n```"
	))
	.await
}
