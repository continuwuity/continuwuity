use std::{fmt, time::SystemTime};

use conduwuit::Result;
use conduwuit_service::Services;
use futures::{
	Future, FutureExt, TryFutureExt,
	io::{AsyncWriteExt, BufWriter},
	lock::Mutex,
};
use ruma::{EventId, UserId};

pub(crate) struct Context<'a> {
	pub(crate) services: &'a Services,
	pub(crate) body: &'a [&'a str],
	pub(crate) timer: SystemTime,
	pub(crate) reply_id: Option<&'a EventId>,
	pub(crate) sender: Option<&'a UserId>,
	pub(crate) output: Mutex<BufWriter<Vec<u8>>>,
}

impl Context<'_> {
	pub(crate) fn write_fmt(
		&self,
		arguments: fmt::Arguments<'_>,
	) -> impl Future<Output = Result> + Send + '_ + use<'_> {
		let buf = format!("{arguments}");
		self.output.lock().then(async move |mut output| {
			output.write_all(buf.as_bytes()).map_err(Into::into).await
		})
	}

	pub(crate) fn write_str<'a>(
		&'a self,
		s: &'a str,
	) -> impl Future<Output = Result> + Send + 'a {
		self.output.lock().then(async move |mut output| {
			output.write_all(s.as_bytes()).map_err(Into::into).await
		})
	}

	/// Get the sender as a string, or service user ID if not available
	pub(crate) fn sender_or_service_user(&self) -> &UserId {
		self.sender
			.unwrap_or_else(|| self.services.globals.server_user.as_ref())
	}
}
