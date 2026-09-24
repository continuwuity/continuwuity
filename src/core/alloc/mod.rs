//! Integration with allocators

// jemalloc
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub mod je;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub use je::{memory_stats, memory_usage, memory_usage_stats, trim};

#[cfg(all(not(target_env = "msvc"), feature = "hardened_malloc", not(feature = "jemalloc")))]
pub mod hardened;
#[cfg(all(
	not(target_env = "msvc"),
	feature = "hardened_malloc",
	not(feature = "jemalloc")
))]
pub use hardened::{memory_stats, memory_usage, memory_usage_stats, trim};

#[cfg(any(
	target_env = "msvc",
	all(not(feature = "hardened_malloc"), not(feature = "jemalloc"))
))]
pub mod default;
#[cfg(any(
	target_env = "msvc",
	all(not(feature = "hardened_malloc"), not(feature = "jemalloc"))
))]
pub use default::{memory_stats, memory_usage, memory_usage_stats, trim};
