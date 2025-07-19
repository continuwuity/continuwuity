use std::sync::Arc;

use super::Capture;
use crate::SyncRwLock;

/// Capture layer state.
pub struct State {
	pub(super) active: SyncRwLock<Vec<Arc<Capture>>>,
}

impl Default for State {
	fn default() -> Self { Self::new() }
}

impl State {
	#[must_use]
	pub fn new() -> Self { Self { active: SyncRwLock::new(Vec::new()) } }

	pub(super) fn add(&self, capture: &Arc<Capture>) {
		self.active.write().push(capture.clone());
	}

	pub(super) fn del(&self, capture: &Arc<Capture>) {
		let mut vec = self.active.write();
		if let Some(pos) = vec.iter().position(|v| Arc::ptr_eq(v, capture)) {
			vec.swap_remove(pos);
		}
	}
}
