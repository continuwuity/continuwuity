use conduwuit::{debug, error, warn};
use rocksdb::LogLevel;

#[tracing::instrument(
	parent = None,
	name = "rocksdb",
	level = "trace",
	skip(msg),
)]
pub(crate) fn handle(level: LogLevel, msg: &str) {
	let msg = msg.trim();
	if msg.starts_with("Options") {
		return;
	}

	match level {
		| LogLevel::Header | LogLevel::Debug => debug!("{msg}"),
		| LogLevel::Error | LogLevel::Fatal => error!("{msg}"),
		| LogLevel::Info => debug!("{msg}"),
		| LogLevel::Warn => warn!("{msg}"),
	}
}
