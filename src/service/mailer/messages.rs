use askama::Template;
use ruma::UserId;

pub trait MessageTemplate: Template {
	fn subject(&self) -> String;
}

#[derive(Template)]
#[template(path = "mail/change_email.txt")]
pub struct ChangeEmail<'a> {
	pub user_id: &'a UserId,
	pub verification_link: String,
}

impl MessageTemplate for ChangeEmail<'_> {
	fn subject(&self) -> String { "Verify your email address".to_owned() }
}

#[derive(Template)]
#[template(path = "mail/new_account.txt")]
pub struct NewAccount<'a> {
	pub server_name: &'a str,
	pub verification_link: String,
}

impl MessageTemplate for NewAccount<'_> {
	fn subject(&self) -> String { "Create your new Matrix account".to_owned() }
}

#[derive(Template)]
#[template(path = "mail/password_reset.txt")]
pub struct PasswordReset<'a> {
	pub display_name: Option<&'a str>,
	pub user_id: &'a UserId,
	pub verification_link: String,
}

impl MessageTemplate for PasswordReset<'_> {
	fn subject(&self) -> String { format!("Password reset request for {}", &self.user_id) }
}

#[derive(Template)]
#[template(path = "mail/test.txt")]
pub struct Test;

impl MessageTemplate for Test {
	fn subject(&self) -> String { "Test message".to_owned() }
}
