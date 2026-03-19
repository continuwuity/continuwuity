use askama::Template;
use ruma::UserId;

pub trait MessageTemplate: Template {
	fn subject(&self) -> String;
}

#[derive(Template)]
#[template(path = "mail/change_email.txt.j2")]
pub struct ChangeEmail<'a> {
	user_id: &'a UserId,
	verification_link: &'a str,
}

impl MessageTemplate for ChangeEmail<'_> {
	fn subject(&self) -> String { "Verify your email address".to_owned() }
}

#[derive(Template)]
#[template(path = "mail/new_account.txt.j2")]
pub struct NewAccount<'a> {
	server_name: &'a str,
	verification_link: &'a str,
}

impl MessageTemplate for NewAccount<'_> {
	fn subject(&self) -> String { "Create your new Matrix account".to_owned() }
}

#[derive(Template)]
#[template(path = "mail/password_reset.txt.j2")]
pub struct PasswordReset<'a> {
	display_name: &'a str,
	user_id: &'a UserId,
	verification_link: &'a str,
}

impl MessageTemplate for PasswordReset<'_> {
	fn subject(&self) -> String { format!("Password reset request for {}", &self.user_id) }
}

#[derive(Template)]
#[template(path = "mail/test.txt.j2")]
pub struct Test;

impl MessageTemplate for Test {
	fn subject(&self) -> String { "Test message".to_owned() }
}
