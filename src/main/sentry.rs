#![cfg(feature = "sentry_telemetry")]

use std::{
	borrow::Cow,
	str::FromStr,
	sync::{Arc, OnceLock},
};

use conduwuit_build_metadata as build;
use conduwuit_core::{config::Config, debug, trace};
use sentry::{
	Breadcrumb, ClientOptions, Level,
	types::{
		Dsn,
		protocol::v7::{Context, Event},
	},
};

static SEND_PANIC: OnceLock<bool> = OnceLock::new();
static SEND_ERROR: OnceLock<bool> = OnceLock::new();

pub(crate) fn init(config: &Config) -> Option<sentry::ClientInitGuard> {
	config.sentry.then(|| sentry::init(options(config)))
}

fn options(config: &Config) -> ClientOptions {
	SEND_PANIC
		.set(config.sentry_send_panic)
		.expect("SEND_PANIC was not previously set");
	SEND_ERROR
		.set(config.sentry_send_error)
		.expect("SEND_ERROR was not previously set");

	let dsn = config
		.sentry_endpoint
		.as_ref()
		.expect("init_sentry should only be called if sentry is enabled and this is not None")
		.as_str();

	let server_name = config
		.sentry_send_server_name
		.then(|| config.server_name.to_string().into());

	ClientOptions {
		dsn: Some(Dsn::from_str(dsn).expect("sentry_endpoint must be a valid URL")),
		server_name,
		traces_sample_rate: config.sentry_traces_sample_rate,
		debug: cfg!(debug_assertions),
		release: release_name(),
		user_agent: conduwuit_core::version::user_agent().into(),
		attach_stacktrace: config.sentry_attach_stacktrace,
		before_send: Some(Arc::new(before_send)),
		before_breadcrumb: Some(Arc::new(before_breadcrumb)),
		..Default::default()
	}
}

fn before_send(event: Event<'static>) -> Option<Event<'static>> {
	if event.exception.iter().any(|e| e.ty == "panic") && !SEND_PANIC.get().unwrap_or(&true) {
		return None;
	}

	if event.level == Level::Error {
		if !SEND_ERROR.get().unwrap_or(&true) {
			return None;
		}

		if cfg!(debug_assertions) {
			return None;
		}

		//NOTE: we can enable this to specify error!(sentry = true, ...)
		if let Some(Context::Other(context)) = event.contexts.get("Rust Tracing Fields") {
			if !context.contains_key("sentry") {
				//return None;
			}
		}
	}

	if event.level == Level::Fatal {
		trace!("{event:#?}");
	}

	debug!("Sending sentry event: {event:?}");
	Some(event)
}

fn before_breadcrumb(crumb: Breadcrumb) -> Option<Breadcrumb> {
	if crumb.ty == "log" && crumb.level == Level::Debug {
		return None;
	}

	trace!("Sentry breadcrumb: {crumb:?}");
	Some(crumb)
}

fn release_name() -> Option<Cow<'static, str>> {
	static RELEASE: OnceLock<Option<String>> = OnceLock::new();

	RELEASE
		.get_or_init(|| {
			let pkg_name = env!("CARGO_PKG_NAME");
			let pkg_version = env!("CARGO_PKG_VERSION");

			if let Some(commit_short) = build::GIT_COMMIT_HASH_SHORT {
				Some(format!("{pkg_name}@{pkg_version}+{commit_short}"))
			} else {
				Some(format!("{pkg_name}@{pkg_version}"))
			}
		})
		.as_ref()
		.map(|s| Cow::Borrowed(s.as_str()))
}
