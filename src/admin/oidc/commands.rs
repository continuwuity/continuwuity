use conduwuit::Result;

use crate::utils::parse_active_local_user_id;

impl crate::Context<'_> {
	pub(super) async fn oidc_link(&self, user_id: String, subject: String) -> Result {
		let user_id = parse_active_local_user_id(self.services, &user_id).await?;

		self.services.oidc.link_user(&user_id, &subject).await;

		self.write_str("Account linked successfully").await?;

		Ok(())
	}

	pub(super) async fn oidc_unlink(&self, _user_id: String) -> Result { todo!() }
}
