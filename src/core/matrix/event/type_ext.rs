use ruma::events::{StateEventType, TimelineEventType};

use super::StateKey;

/// Convenience trait for adding event type plus state key to state maps.
pub trait TypeExt {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey);
}

impl TypeExt for StateEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self, state_key.into())
	}
}

impl TypeExt for &StateEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self.clone(), state_key.into())
	}
}

impl TypeExt for TimelineEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self.into(), state_key.into())
	}
}

impl TypeExt for &TimelineEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self.clone().into(), state_key.into())
	}
}
