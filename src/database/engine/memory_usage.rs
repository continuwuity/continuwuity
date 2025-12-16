use std::fmt::Write;

use conduwuit::{Result, implement};
use rocksdb::perf::MemoryUsageBuilder;

use super::Engine;
use crate::or_else;

#[implement(Engine)]
pub fn memory_usage(&self) -> Result<String> {
	let mut res = String::new();

	let mut builder = MemoryUsageBuilder::new().or_else(or_else)?;
	builder.add_db(&self.db);
	builder.add_cache(&self.ctx.row_cache.lock());

	let usage = builder.build().or_else(or_else)?;

	let mibs = |input| f64::from(u32::try_from(input / 1024).unwrap_or(0)) / 1024.0;
	writeln!(
		res,
		"Memory buffers: {:.2} MiB\nPending write: {:.2} MiB\nTable readers: {:.2} MiB\nRow \
		 cache: {:.2} MiB",
		mibs(usage.approximate_mem_table_total()),
		mibs(usage.approximate_mem_table_unflushed()),
		mibs(usage.approximate_mem_table_readers_total()),
		mibs(u64::try_from(self.ctx.row_cache.lock().get_usage())?),
	)?;

	for (name, cache) in &*self.ctx.col_cache.lock() {
		writeln!(res, "{name} cache: {:.2} MiB", mibs(u64::try_from(cache.get_usage())?))?;
	}

	Ok(res)
}
