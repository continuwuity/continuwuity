use std::sync::Arc;

use conduwuit::{Err, Result, err, info};
use lettre::{
	AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
	message::{Mailbox, MessageBuilder},
};

use crate::{Args, mailer::messages::MessageTemplate};

pub mod messages;

type Transport = AsyncSmtpTransport<Tokio1Executor>;
type TransportError = lettre::transport::smtp::Error;

pub struct Service {
	transport: Option<(Mailbox, Transport)>,
}

#[async_trait::async_trait]
impl crate::Service for Service {
	fn build(args: Args<'_>) -> Result<Arc<Self>> {
		let transport = args
			.server
			.config
			.smtp
			.as_ref()
			.map(|config| {
				Ok((config.sender.clone(), Transport::from_url(&config.connection_uri)?.build()))
			})
			.transpose()
			.map_err(|err: TransportError| err!("Failed to set up SMTP transport: {err}"))?;

		Ok(Arc::new(Self { transport }))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	async fn worker(self: Arc<Self>) -> Result<()> {
		if let Some((_, ref transport)) = self.transport {
			match transport.test_connection().await {
				| Ok(true) => {
					info!("SMTP connection test successful");
					Ok(())
				},
				| Ok(false) => {
					Err!("SMTP connection test failed")
				},
				| Err(err) => {
					Err!("SMTP connection test failed: {err}")
				},
			}
		} else {
			info!("SMTP is not configured, email functionality will be unavailable");
			Ok(())
		}
	}
}

impl Service {
	/// Returns a mailer which allows email to be sent, if SMTP is configured.
	#[must_use]
	pub fn mailer(&self) -> Option<Mailer<'_>> {
		self.transport
			.as_ref()
			.map(|(sender, transport)| Mailer { sender, transport })
	}
}

pub struct Mailer<'a> {
	sender: &'a Mailbox,
	transport: &'a Transport,
}

impl Mailer<'_> {
	/// Sends an email.
	pub async fn send<Template: MessageTemplate>(
		&self,
		recipient: Mailbox,
		message: Template,
	) -> Result<()> {
		let subject = message.subject();
		let body = message
			.render()
			.map_err(|err| err!("Failed to render message template: {err}"))?;

		let message = MessageBuilder::new()
			.from(self.sender.clone())
			.to(recipient)
			.subject(subject)
			.date_now()
			.body(body)
			.expect("should have been able to construct message");

		self.transport
			.send(message)
			.await
			.map_err(|err: TransportError| err!("Failed to send message: {err}"))?;

		Ok(())
	}
}
