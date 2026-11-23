#![type_length_limit = "49152"] //TODO: reduce me

use std::sync::{Arc, atomic::Ordering};

use conduwuit_core::{debug_info, error};

conduwuit_macros::introspect_crate! {}

mod clap;
mod deadlock;
mod logging;
mod metrics;
mod panic;
mod restart;
mod runtime;
mod sentry;
mod server;
mod signal;

pub use conduwuit_core::{Error, Result};
use server::Server;

pub use crate::clap::Args;

pub fn run() -> Result<()> {
	panic::init();

	let args = clap::parse();
	run_with_args(&args)
}

pub fn run_with_args(args: &Args) -> Result<()> {
	// Spawn deadlock detection thread
	deadlock::spawn();
	#[cfg(not(any(feature = "jemalloc", feature = "mimalloc")))]
	conduwuit_core::warn!(
		"Your build configuration does not have a non-system allocator enabled. This may result \
		 in decreased performance."
	);

	// Because we're not using rustls default-tls, we have to initialise a TLS
	// provider
	#[cfg(feature = "aws_lc_rs")]
	rustls::crypto::aws_lc_rs::default_provider()
		.install_default()
		.expect("failed to initialise ring rustls crypto provider");

	#[cfg(all(feature = "ring", not(feature = "aws_lc_rs")))]
	rustls::crypto::ring::default_provider()
		.install_default()
		.expect("failed to initialise ring rustls crypto provider");

	let runtime = runtime::new(args)?;
	let server = Server::new(args, Some(runtime.handle()))?;

	runtime.spawn(signal::signal(server.clone()));
	runtime.block_on(async_main(&server))?;
	server.shutdown_metrics();
	runtime::shutdown(&server, runtime);

	#[cfg(unix)]
	if server.server.restarting.load(Ordering::Acquire) {
		restart::restart();
	}

	debug_info!("Exit");
	Ok(())
}

/// Operate the server. This will start, run and stop the server within the
/// asynchronous runtime.
#[tracing::instrument(
	name = "main",
	parent = None,
	skip_all,
	level = "info"
)]
async fn async_main(server: &Arc<Server>) -> Result<(), Error> {
	extern crate conduwuit_router as router;

	match router::start(&server.server).await {
		| Ok(services) => server.services.lock().await.insert(services),
		| Err(error) => {
			error!("Critical error starting server: {error}");
			return Err(error);
		},
	};

	if let Err(error) = router::run(
		server
			.services
			.lock()
			.await
			.as_ref()
			.expect("services initialized"),
	)
	.await
	{
		error!("Critical error running server: {error}");
		return Err(error);
	}

	if let Err(error) = router::stop(
		server
			.services
			.lock()
			.await
			.take()
			.expect("services initialized"),
	)
	.await
	{
		error!("Critical error stopping server: {error}");
		return Err(error);
	}

	debug_info!("Exit runtime");
	Ok(())
}
