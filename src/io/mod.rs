//! I/O subsystem: request types and backends.

pub mod aio;
pub mod backend;
pub(crate) mod pool;
pub mod request;
pub(crate) mod types;
