use conduwuit::utils;
use database::{Deserialized, Json};
use ruma::{UserId, api::client::filter::FilterDefinition};

impl super::Service {
	/// Creates a new sync filter. Returns the filter id.
	pub fn create_filter(&self, user_id: &UserId, filter: &FilterDefinition) -> String {
		let filter_id = utils::random_string(4);

		// TODO: filters should be de-duplicated and also not per-user
		let key = (user_id, &filter_id);
		self.db.userfilterid_filter.put(key, Json(filter));

		filter_id
	}

	/// Fetches a filter from a filter ID belonging to a user.
	pub async fn get_filter(
		&self,
		user_id: &UserId,
		filter_id: &str,
	) -> conduwuit::Result<FilterDefinition> {
		let key = (user_id, filter_id);
		self.db.userfilterid_filter.qry(&key).await.deserialized()
	}
}
