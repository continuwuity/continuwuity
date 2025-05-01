use std::{ops::Deref, sync::Arc};

use crate::Services;

#[derive(Clone, Copy)]
pub struct State {
	services: *const Services,
}

pub struct Guard {
	services: Arc<Services>,
}

pub fn create(services: Arc<Services>) -> (State, Guard) {
	let state = State {
		services: Arc::into_raw(services.clone()),
	};

	let guard = Guard { services };

	(state, guard)
}

impl Drop for Guard {
	fn drop(&mut self) {
		let ptr = Arc::as_ptr(&self.services);
		// SAFETY: Parity with Arc::into_raw() called in create(). This revivifies the
		// Arc lost to State so it can be dropped, otherwise Services will leak.
		let arc = unsafe { Arc::from_raw(ptr) };
		debug_assert!(
			Arc::strong_count(&arc) > 1,
			"Services usually has more than one reference and is not dropped here"
		);
	}
}

impl Deref for State {
	type Target = Services;

	fn deref(&self) -> &Self::Target {
		deref(&self.services).expect("dereferenced Services pointer in State must not be null")
	}
}

/// SAFETY: State is a thin wrapper containing a raw const pointer to Services
/// in lieu of an Arc. Services is internally threadsafe. If State contains
/// additional fields this notice should be reevaluated.
unsafe impl Send for State {}

/// SAFETY: State is a thin wrapper containing a raw const pointer to Services
/// in lieu of an Arc. Services is internally threadsafe. If State contains
/// additional fields this notice should be reevaluated.
unsafe impl Sync for State {}

fn deref(services: &*const Services) -> Option<&Services> {
	// SAFETY: We replaced Arc<Services> with *const Services in State. This is
	// worth about 10 clones (20 reference count updates) for each request handled.
	// Though this is not an incredibly large quantity, it's woefully unnecessary
	// given the context as explained below; though it is not currently known to be
	// a performance bottleneck, the front-line position justifies preempting it.
	//
	// Services is created prior to the axum/tower stack and Router, and prior
	// to serving any requests through the handlers in this crate. It is then
	// dropped only after all requests have completed, the listening sockets
	// have been closed, axum/tower has been dropped. Thus Services is
	// expected to live at least as long as any instance of State, making the
	// constant updates to the prior Arc unnecessary to keep Services alive.
	//
	// Nevertheless if it is possible to accomplish this by annotating State
	// with a lifetime to hold a reference (and be aware I have made a
	// significant effort trying to make this work) this unsafety may not be
	// necessary. It is either very difficult or impossible to get a
	// lifetime'ed reference through Router / RumaHandler; though it is
	// possible to pass a reference through axum's `with_state()` in trivial
	// configurations as the only requirement of a State is Clone.
	unsafe { services.as_ref() }
}
