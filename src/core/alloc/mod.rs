//! Integration with allocators

// jemalloc
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub mod je;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub(crate) use je::metric;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
pub use je::{memory_stats, memory_usage, memory_usage_stats, trim};

// mimalloc
#[cfg(all(feature = "mimalloc", feature = "jemalloc", not(target_env = "msvc")))]
compile_error!("the `mimalloc` and `jemalloc` features are mutually exclusive");
#[cfg(all(feature = "mimalloc", any(target_env = "msvc", not(feature = "jemalloc"))))]
pub mod mimalloc;
#[cfg(all(feature = "mimalloc", any(target_env = "msvc", not(feature = "jemalloc"))))]
pub(crate) use mimalloc::metric;
#[cfg(all(feature = "mimalloc", any(target_env = "msvc", not(feature = "jemalloc"))))]
pub use mimalloc::{memory_stats, memory_usage, memory_usage_stats, trim};

// default
#[cfg(all(not(feature = "mimalloc"), any(target_env = "msvc", not(feature = "jemalloc"))))]
pub mod default;
#[cfg(all(not(feature = "mimalloc"), any(target_env = "msvc", not(feature = "jemalloc"))))]
pub(crate) use default::metric;
#[cfg(all(not(feature = "mimalloc"), any(target_env = "msvc", not(feature = "jemalloc"))))]
pub use default::{memory_stats, memory_usage, memory_usage_stats, trim};
