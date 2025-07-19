use std::fmt::Write;

use conduwuit::{Result, implement};
use rocksdb::perf::get_memory_usage_stats;

use super::Engine;
use crate::or_else;

#[implement(Engine)]
pub fn memory_usage(&self) -> Result<String> {
	let mut res = String::new();
	let stats = get_memory_usage_stats(Some(&[&self.db]), Some(&[&*self.ctx.row_cache.lock()]))
		.or_else(or_else)?;
	let mibs = |input| f64::from(u32::try_from(input / 1024).unwrap_or(0)) / 1024.0;
	writeln!(
		res,
		"Memory buffers: {:.2} MiB\nPending write: {:.2} MiB\nTable readers: {:.2} MiB\nRow \
		 cache: {:.2} MiB",
		mibs(stats.mem_table_total),
		mibs(stats.mem_table_unflushed),
		mibs(stats.mem_table_readers_total),
		mibs(u64::try_from(self.ctx.row_cache.lock().get_usage())?),
	)?;

	for (name, cache) in &*self.ctx.col_cache.lock() {
		writeln!(res, "{name} cache: {:.2} MiB", mibs(u64::try_from(cache.get_usage())?))?;
	}

	Ok(res)
}
