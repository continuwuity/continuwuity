#![type_length_limit = "49152"] //TODO: reduce me

use std::sync::{Arc, atomic::Ordering};

use conduwuit_core::{debug_info, error};

mod clap;
mod logging;
mod mods;
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
	let runtime = runtime::new(args)?;
	let server = Server::new(args, Some(runtime.handle()))?;

	runtime.spawn(signal::signal(server.clone()));
	runtime.block_on(async_main(&server))?;
	runtime::shutdown(&server, runtime);

	#[cfg(unix)]
	if server.server.restarting.load(Ordering::Acquire) {
		restart::restart();
	}

	debug_info!("Exit");
	Ok(())
}

/// Operate the server normally in release-mode static builds. This will start,
/// run and stop the server within the asynchronous runtime.
#[cfg(any(not(conduwuit_mods), not(feature = "conduwuit_mods")))]
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

/// Operate the server in developer-mode dynamic builds. This will start, run,
/// and hot-reload portions of the server as-needed before returning for an
/// actual shutdown. This is not available in release-mode or static builds.
#[cfg(all(conduwuit_mods, feature = "conduwuit_mods"))]
async fn async_main(server: &Arc<Server>) -> Result<(), Error> {
	let mut starts = true;
	let mut reloads = true;
	while reloads {
		if let Err(error) = mods::open(server).await {
			error!("Loading router: {error}");
			return Err(error);
		}

		let result = mods::run(server, starts).await;
		if let Ok(result) = result {
			(starts, reloads) = result;
		}

		let force = !reloads || result.is_err();
		if let Err(error) = mods::close(server, force).await {
			error!("Unloading router: {error}");
			return Err(error);
		}

		if let Err(error) = result {
			error!("{error}");
			return Err(error);
		}
	}

	debug_info!("Exit runtime");
	Ok(())
}
