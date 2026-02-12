use std::sync::{
	Arc, OnceLock,
	atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use conduwuit::Result;
use futures::StreamExt;

use crate::{Dep, config, users};

pub struct Service {
	services: Services,
	/// Represents the state of first run mode.
	///
	/// First run mode is either active or inactive at server start. It may
	/// transition from active to inactive, but only once, and can never
	/// transition the other way. Additionally, whether the server is in first
	/// run mode or not can only be determined when all services are
	/// constructed. The outer `OnceLock` represents the unknown state of first
	/// run mode, and the inner `OnceLock` enforces the one-time transition from
	/// active to inactive.
	///
	/// Consequently, this marker may be in one of three states:
	/// 1. OnceLock<uninitialized>, representing the unknown state of first run
	///    mode during server startup. Once server startup is complete, the
	///    marker transitions to state 2 or directly to state 3.
	/// 2. OnceLock<OnceLock<uninitialized>>, representing first run mode being
	///    active. The marker may only transition to state 3 from here.
	/// 3. OnceLock<OnceLock<()>>, representing first run mode being inactive.
	///    The marker may not transition out of this state.
	first_run_marker: OnceLock<OnceLock<()>>,
}

struct Services {
	config: Dep<config::Service>,
	users: Dep<users::Service>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
				users: args.depend::<users::Service>("users"),
			},
			// marker starts in an indeterminate state
			first_run_marker: OnceLock::new(),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	async fn worker(self: Arc<Self>) -> Result {
		// first run mode will be enabled if there are no local users
		let is_first_run = self
			.services
			.users
			.list_local_users()
			.next()
			.await
			.is_none();

		self.first_run_marker
			.set(if is_first_run {
				// first run mode is active (empty inner lock)
				OnceLock::new()
			} else {
				// first run mode is inactive (already filled inner lock)
				OnceLock::from(())
			})
			.expect("Service worker should only be called once");

		Ok(())
	}
}

impl Service {
	/// Check if first run mode is active.
	pub fn is_first_run(&self) -> bool {
		self.first_run_marker
			.get()
			.expect("First run mode should not be checked during server startup")
			.get()
			.is_none()
	}

	/// Disable first run mode and begin normal operation.
	pub fn disable_first_run(&self) {
		self.first_run_marker
			.get()
			.expect("First run mode should not be disabled during server startup")
			.set(())
			.expect("First run mode should not be disabled more than once");
	}

	pub(crate) fn print_banner(&self) {
		// This function is specially called by the core after all other
		// services have started. It runs last to ensure that the banner it
		// prints comes after any other logging which may occur on startup.

		if !self.is_first_run() {
			return;
		}

		println!("meow");
	}
}
