use std::collections::BTreeMap;

use crate::config::{Experiments, experiments::ToggledExperiment};

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
		"v1.19".to_owned(),
	]
}

#[must_use]
pub fn unstable_features(experiments: &Experiments) -> BTreeMap<String, bool> {
	BTreeMap::from_iter([
		// MatrixRTC (https://github.com/matrix-org/matrix-spec-proposals/pull/4143)
		// Expected for spec v1.20
		("org.matrix.msc4143".to_owned(), true),
		// Simplified Sliding sync (https://github.com/matrix-org/matrix-spec-proposals/pull/4186)
		// Expected for spec v1.20
		("org.matrix.simplified_msc3575".to_owned(), true),
		("org.matrix.msc4155".to_owned(), experiments.msc4155.is_enabled()),
		// profile change propagation (https://github.com/matrix-org/matrix-spec-proposals/pull/4466)
		("computer.gingershaped.msc4466".to_owned(), true),
		("org.matrix.msc4174".to_owned(), experiments.msc4174.is_enabled()),
		("uk.timedout.msc4494".to_owned(), experiments.msc4494.is_enabled()),
		("org.continuwuity.msc4484.unstable".to_owned(), experiments.msc4484.is_enabled()),
		("org.continuwuity.msc4560".to_owned(), experiments.msc4560.is_enabled()),
		("fi.mau.msc2815".to_owned(), experiments.msc2815.is_enabled()),
	])
}
