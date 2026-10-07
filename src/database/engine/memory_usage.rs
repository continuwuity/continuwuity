use std::fmt::Write;

use conduwuit::Result;
use rocksdb::perf::get_memory_usage_stats;

use crate::or_else;

pub struct MemoryUsage {
	pub mem_table_total: u64,
	pub mem_table_unflushed: u64,
	pub mem_table_readers_total: u64,
	pub row_cache: u64,
}

impl super::Engine {
	pub fn memory_usage(&self) -> Result<String> {
		let mut res = String::new();
		let stats = self.memory_usage_stats()?;
		let col_caches = self.col_cache_usage()?;
		let mibs = |input| f64::from(u32::try_from(input / 1024).unwrap_or(0)) / 1024.0;
		writeln!(
			res,
			"Memory buffers: {:.2} MiB\nPending write: {:.2} MiB\nTable readers: {:.2} MiB\nRow \
			 cache: {:.2} MiB",
			mibs(stats.mem_table_total),
			mibs(stats.mem_table_unflushed),
			mibs(stats.mem_table_readers_total),
			mibs(stats.row_cache),
		)?;

		for (name, usage) in col_caches {
			writeln!(res, "{name} cache: {:.2} MiB", mibs(usage))?;
		}

		Ok(res)
	}

	pub fn memory_usage_stats(&self) -> Result<MemoryUsage> {
		let stats =
			get_memory_usage_stats(Some(&[&self.db]), Some(&[&*self.ctx.row_cache.lock()]))
				.or_else(or_else)?;

		Ok(MemoryUsage {
			mem_table_total: stats.mem_table_total,
			mem_table_unflushed: stats.mem_table_unflushed,
			mem_table_readers_total: stats.mem_table_readers_total,
			row_cache: u64::try_from(self.ctx.row_cache.lock().get_usage())?,
		})
	}

	pub fn col_cache_usage(&self) -> Result<Vec<(String, u64)>> {
		self.ctx
			.col_cache
			.lock()
			.iter()
			.map(|(name, cache)| Ok((name.clone(), u64::try_from(cache.get_usage())?)))
			.collect()
	}
}
