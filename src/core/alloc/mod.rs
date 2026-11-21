//! Integration with allocators

pub mod default;

#[cfg(feature = "jemalloc")]
pub mod je;
#[cfg(feature = "jemalloc")]
pub use je::{memory_stats, memory_usage, memory_usage_stats, trim};

#[cfg(all(feature = "mimalloc", not(feature = "jemalloc")))]
pub mod mimalloc;

#[cfg(not(feature = "jemalloc"))]
pub use default::{memory_stats, memory_usage, memory_usage_stats, trim};
