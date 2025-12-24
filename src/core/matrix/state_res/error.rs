use ruma::OwnedEventId;
use serde_json::Error as JsonError;
use thiserror::Error;

/// Represents the various errors that arise when resolving state.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
	/// A deserialization error.
	#[error(transparent)]
	SerdeJson(#[from] JsonError),

	/// The given option or version is unsupported.
	#[error("Unsupported room version: {0}")]
	Unsupported(String),

	/// The given event was not found.
	#[error("Event not found: {0}")]
	NotFound(String),

	/// A required event this event depended on could not be fetched,
	/// either as it was missing, or because it was invalid
	#[error("Failed to fetch required {0} event: {1}")]
	DependencyFailed(OwnedEventId, String),

	/// Invalid fields in the given PDU.
	#[error("Invalid PDU: {0}")]
	InvalidPdu(String),

	/// This event failed an authorization condition.
	#[error("Auth check failed: {0}")]
	AuthConditionFailed(String),

	/// This event contained multiple auth events of the same type and state
	/// key.
	#[error("Duplicate auth events: {0}")]
	DuplicateAuthEvents(String),

	/// This event contains unnecessary auth events.
	#[error("Unknown or unnecessary auth events present: {0}")]
	UnselectedAuthEvents(String),
}
