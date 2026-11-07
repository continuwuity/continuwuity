use ruma::events::relation::RelationType;
use serde::Deserialize;

use super::Event;

pub trait RelationTypeEqual<E: Event> {
	fn relation_type_equal(&self, event: &E) -> bool;
}

#[derive(Clone, Debug, Deserialize)]
struct ExtractRelatesToRelType {
	#[serde(rename = "m.relates_to")]
	relates_to: ExtractRelType,
}

#[derive(Clone, Debug, Deserialize)]
struct ExtractRelType {
	rel_type: RelationType,
}

/// The type of an event's relation, if it has one
pub fn relation_type<E: Event>(event: &E) -> Option<RelationType> {
	event
		.get_content()
		.map(|c: ExtractRelatesToRelType| c.relates_to.rel_type)
		.ok()
}

impl<E: Event> RelationTypeEqual<E> for RelationType {
	fn relation_type_equal(&self, event: &E) -> bool {
		relation_type(event).is_some_and(|r| r == *self)
	}
}
