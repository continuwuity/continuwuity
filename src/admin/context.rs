use std::{fmt, time::SystemTime};

use conduwuit::{Err, Result};
use conduwuit_service::Services;
use futures::{
	Future, FutureExt, TryFutureExt,
	io::{AsyncWriteExt, BufWriter},
	lock::Mutex,
};
use ruma::{EventId, UserId};
use service::admin::InvocationSource;

pub(crate) struct Context<'a> {
	pub(crate) services: &'a Services,
	pub(crate) body: &'a [&'a str],
	pub(crate) timer: SystemTime,
	pub(crate) reply_id: Option<&'a EventId>,
	pub(crate) sender: Option<&'a UserId>,
	pub(crate) output: Mutex<BufWriter<Vec<u8>>>,
	pub(crate) source: InvocationSource,
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

	/// Returns an Err if the [`Self::source`] of this context does not allow
	/// restricted commands to be executed.
	///
	/// This is intended to be placed at the start of restricted commands'
	/// implementations, like so:
	///
	/// ```ignore
	/// self.bail_restricted()?;
	/// // actual command impl
	/// ```
	pub(crate) fn bail_restricted(&self) -> Result {
		if self.source.allows_restricted() {
			Ok(())
		} else {
			Err!("This command can only be used in the admin room.")
		}
	}
}
