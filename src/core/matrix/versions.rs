use std::collections::BTreeMap;

#[must_use]
pub fn versions() -> Vec<String> {
	vec![
		"r0.0.1".to_owned(),
		"r0.1.0".to_owned(),
		"r0.2.0".to_owned(),
		"r0.3.0".to_owned(),
		"r0.4.0".to_owned(),
		"r0.5.0".to_owned(),
		"r0.6.0".to_owned(),
		"r0.6.1".to_owned(),
		"v1.1".to_owned(),
		"v1.2".to_owned(),
		"v1.3".to_owned(),
		"v1.4".to_owned(),
		"v1.5".to_owned(),
		"v1.8".to_owned(),
		"v1.11".to_owned(),
		"v1.12".to_owned(),
		"v1.13".to_owned(),
		"v1.14".to_owned(),
		"v1.15".to_owned(),
		"v1.16".to_owned(),
		"v1.17".to_owned(),
		"v1.18".to_owned(),
	]
}

#[must_use]
pub fn unstable_features() -> BTreeMap<String, bool> {
	BTreeMap::from_iter([
		// query mutual rooms (https://github.com/matrix-org/matrix-spec-proposals/pull/2666)
		// Expected for spec v1.19
		("uk.half-shot.msc2666.query_mutual_rooms".to_owned(), true),
		// Simplified Sliding sync (https://github.com/matrix-org/matrix-spec-proposals/pull/4186)
		// Expected for spec v1.19
		("org.matrix.simplified_msc3575".to_owned(), true),
		// invite filtering (https://github.com/matrix-org/matrix-spec-proposals/pull/4155)
		("org.matrix.msc4155".to_owned(), true),
		// profile change propagation (https://github.com/matrix-org/matrix-spec-proposals/pull/4466)
		("computer.gingershaped.msc4466".to_owned(), true),
		// server admin oauth scope (https://github.com/matrix-org/matrix-spec-proposals/pull/4484)
		("org.continuwuity.msc4484.unstable".to_owned(), true),
	])
}
