use conduwuit::Result;

use crate::utils::parse_active_local_user_id;

impl crate::Context<'_> {
	pub(super) async fn oidc_link(&self, user_id: String, subject: String) -> Result {
		let user_id = parse_active_local_user_id(self.services, &user_id).await?;

		self.services.oidc.link_user(&user_id, &subject);

		self.write_str(&format!("Subject `{subject}` linked to account `{user_id}`."))
			.await?;

		Ok(())
	}

	pub(super) async fn oidc_unlink(&self, subject: String) -> Result {
		self.services.oidc.unlink_user(&subject);

		self.write_str(&format!("Subject `{subject}` unlinked."))
			.await?;

		Ok(())
	}
}
