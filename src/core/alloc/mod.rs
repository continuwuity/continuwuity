//! Integration with allocators

// jemalloc
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub mod je;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub use je::{memory_stats, memory_usage, memory_usage_stats, trim};

// mimalloc
#[cfg(all(feature = "mimalloc", any(target_env = "msvc", not(feature = "jemalloc"))))]
pub mod mimalloc;

// default
#[cfg(any(target_env = "msvc", not(feature = "jemalloc")))]
pub mod default;
#[cfg(any(target_env = "msvc", not(feature = "jemalloc")))]
pub use default::{memory_stats, memory_usage, memory_usage_stats, trim};
