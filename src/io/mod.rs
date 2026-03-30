//! I/O subsystem: request types and backends.
//!
//! ## Backend model separation
//!
//! `SyncBackend` (in `backend/`) and `IoBackend` (this module) are
//! intentionally separate interfaces for separate execution models.
//! `SyncBackend` uses a blocking `execute()` model — one call, one result.
//! `IoBackend` uses a submission-and-completion async model: `submit` enqueues
//! a request; `poll`/`wait` harvest completions; `cancel` aborts in-flight
//! work. They are NOT related by inheritance or trait implementation. Do not
//! attempt to unify them under a single trait.

pub mod aio;
pub mod backend;
pub(crate) mod completion;
pub(crate) mod mock;
pub(crate) mod pending;
pub(crate) mod pool;
pub mod request;
pub(crate) mod sockaddr;
pub(crate) mod threadpool;
pub(crate) mod types;
#[cfg(target_os = "linux")]
pub(crate) mod uring;
pub(crate) mod watch;

use crate::io::request::IoRequest;
use crate::value::heap::TableKey;
use crate::value::Value;
use std::collections::BTreeMap;

/// Completion from an async I/O operation.
pub(crate) struct Completion {
    pub(crate) id: u64,
    pub(crate) result: Result<Value, Value>,
}

impl Completion {
    /// Convert to an Elle struct: {:id n :value v :error nil} or {:id n :value nil :error e}
    pub(crate) fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("id".into()), Value::int(self.id as i64));
        match &self.result {
            Ok(v) => {
                fields.insert(TableKey::Keyword("value".into()), *v);
                fields.insert(TableKey::Keyword("error".into()), Value::NIL);
            }
            Err(e) => {
                fields.insert(TableKey::Keyword("value".into()), Value::NIL);
                fields.insert(TableKey::Keyword("error".into()), *e);
            }
        }
        Value::struct_from(fields)
    }
}

/// Async I/O backend trait.
///
/// Implemented by `AsyncBackend` (real I/O via io_uring or thread pool)
/// and `MockBackend` (in-memory, deterministic).
pub(crate) trait IoBackend {
    fn submit(&self, request: &IoRequest) -> Result<u64, String>;
    fn poll(&self) -> Vec<Completion>;
    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String>;
    fn cancel(&self, id: u64) -> Result<(), String>;
}

/// Type-erased async I/O backend, stored as `Value::external("io-backend", ...)`.
///
/// The primitives downcast to this type. The trait dispatch handles
/// routing to AsyncBackend, MockBackend, or any future backend.
pub(crate) struct AnyBackend(pub(crate) Box<dyn IoBackend>);
