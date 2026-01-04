use conduwuit::{Err, Result, implement};
use rocksdb::{BottommostLevelCompaction, CompactOptions};

use crate::keyval::KeyBuf;

#[derive(Clone, Debug, Default)]
pub struct Options {
	/// Key range to start and stop compaction.
	pub range: (Option<KeyBuf>, Option<KeyBuf>),

	/// (None, None) - all levels to all necessary levels
	/// (None, Some(1)) - compact all levels into level 1
	/// (Some(1), None) - compact level 1 into level 1
	/// (Some(_), Some(_) - currently unsupported
	pub level: (Option<usize>, Option<usize>),

	/// run compaction until complete. if false only one pass is made, and the
	/// results of that pass are not further recompacted.
	pub exhaustive: bool,

	/// waits for other compactions to complete, then runs this compaction
	/// exclusively before allowing automatic compactions to resume.
	pub exclusive: bool,
}

#[implement(super::Map)]
#[tracing::instrument(
	name = "compact",
	level = "info",
	skip(self),
	fields(%self),
)]
pub fn compact_blocking(&self, opts: Options) -> Result {
	let mut co = CompactOptions::default();
	co.set_exclusive_manual_compaction(opts.exclusive);
	co.set_bottommost_level_compaction(match opts.exhaustive {
		| true => BottommostLevelCompaction::Force,
		| false => BottommostLevelCompaction::ForceOptimized,
	});

	match opts.level {
		| (None, None) => {
			co.set_change_level(true);
			co.set_target_level(-1);
		},
		| (None, Some(level)) => {
			co.set_change_level(true);
			co.set_target_level(level.try_into()?);
		},
		| (Some(level), None) => {
			co.set_change_level(false);
			co.set_target_level(level.try_into()?);
		},
		| (Some(_), Some(_)) => return Err!("compacting between specific levels not supported"),
	}

	self.db
		.db
		.compact_range_cf_opt(&self.cf(), opts.range.0, opts.range.1, &co);

	Ok(())
}
