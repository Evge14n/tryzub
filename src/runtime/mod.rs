pub mod memory;
pub mod threading;
pub mod error;
pub mod ffi;
pub mod async_runtime;

pub use memory::{MemoryManager, MEMORY_MANAGER};
pub use threading::ThreadPool;
pub use error::{TryzubError, ErrorKind};
pub use ffi::*;
pub use async_runtime::AsyncRuntime;

use once_cell::sync::Lazy;

// Re-export main runtime functions
pub use crate::{
    tryzub_runtime_init,
    tryzub_runtime_shutdown,
    tryzub_allocate,
    tryzub_deallocate,
    tryzub_get_memory_stats,
};
