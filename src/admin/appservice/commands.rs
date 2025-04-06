use conduwuit::{Err, Result, checked};
use futures::{FutureExt, StreamExt, TryFutureExt};

use crate::admin_command;

#[admin_command]
pub(super) async fn register(&self) -> Result {
	let body = &self.body;
	let body_len = self.body.len();
	if body_len < 2
		|| !body[0].trim().starts_with("```")
		|| body.last().unwrap_or(&"").trim() != "```"
	{
		return Err!("Expected code block in command body. Add --help for details.");
	}

	let range = 1..checked!(body_len - 1)?;
	let appservice_config_body = body[range].join("\n");
	let parsed_config = serde_yaml::from_str(&appservice_config_body);
	match parsed_config {
		| Err(e) => return Err!("Could not parse appservice config as YAML: {e}"),
		| Ok(registration) => match self
			.services
			.appservice
			.register_appservice(&registration, &appservice_config_body)
			.await
			.map(|()| registration.id)
		{
			| Err(e) => return Err!("Failed to register appservice: {e}"),
			| Ok(id) => write!(self, "Appservice registered with ID: {id}"),
		},
	}
	.await
}

#[admin_command]
pub(super) async fn unregister(&self, appservice_identifier: String) -> Result {
	match self
		.services
		.appservice
		.unregister_appservice(&appservice_identifier)
		.await
	{
		| Err(e) => return Err!("Failed to unregister appservice: {e}"),
		| Ok(()) => write!(self, "Appservice unregistered."),
	}
	.await
}

#[admin_command]
pub(super) async fn show_appservice_config(&self, appservice_identifier: String) -> Result {
	match self
		.services
		.appservice
		.get_registration(&appservice_identifier)
		.await
	{
		| None => return Err!("Appservice does not exist."),
		| Some(config) => {
			let config_str = serde_yaml::to_string(&config)?;
			write!(self, "Config for {appservice_identifier}:\n\n```yaml\n{config_str}\n```")
		},
	}
	.await
}

#[admin_command]
pub(super) async fn list_registered(&self) -> Result {
	self.services
		.appservice
		.iter_ids()
		.collect()
		.map(Ok)
		.and_then(|appservices: Vec<_>| {
			let len = appservices.len();
			let list = appservices.join(", ");
			write!(self, "Appservices ({len}): {list}")
		})
		.await
}
