//! I/O subsystem: request types and backends.
//!
//! `IoBackend` is the async submission-and-completion model: `submit`
//! enqueues a request; `poll`/`wait` harvest completions; `cancel`
//! aborts in-flight work.  Implemented by `AsyncBackend` (io_uring or
//! thread pool) and `MockBackend` (in-memory, deterministic).

pub mod aio;
pub(crate) mod completion;
pub(crate) mod mock;
pub(crate) mod pending;
pub(crate) mod pool;
pub mod request;
pub(crate) mod sigfd;

/// Install process-wide POSIX signal traps. Called once from
/// `main()` before any thread spawns. Installs sigaction handlers
/// for the terminate (TERM/INT/QUIT/HUP), job-control (TSTP/TTIN/TTOU),
/// and resume (CONT) sets; `SIG_IGN` for SIGPIPE; and
/// `pthread_sigmask(SIG_BLOCK)` for the absorb set (USR1/USR2/CHLD/
/// URG/WINCH/ALRM) on the main thread. See `docs/posix-signals.md`
/// for the full disposition table.
pub fn init_process_signals() {
    sigfd::init_process_signals();
}
pub(crate) mod sigmap;
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

/// Byte offset where the Nth grapheme cluster ends in `buf` (treated
/// as UTF-8).  Used by text-port `ReadExact` to count progress in
/// graphemes — the unit Elle strings are measured in — instead of
/// bytes, so `(port/read-exact text-port 50)` returns a string of
/// `(length 50)` regardless of how many kernel bytes that needed.
///
/// Returns:
/// - `Some(offset)` when at least `n` graphemes have been assembled;
///   `offset` is the byte position one past the Nth grapheme so the
///   caller can split into `buf[..offset]` (the result) and
///   `buf[offset..]` (leftover to stash for the next read).
/// - `None` when `buf` doesn't yet contain `n` graphemes.  This also
///   covers the trailing-partial-codepoint case (final bytes don't
///   form a complete UTF-8 sequence) — the caller resubmits for more
///   bytes.  Mid-buffer invalid UTF-8 is conservatively treated the
///   same way (stop at the last valid prefix); in practice this never
///   fires for well-formed text.
pub(crate) fn grapheme_count_in_valid_prefix(buf: &[u8]) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let valid = match std::str::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            let upto = e.valid_up_to();
            unsafe { std::str::from_utf8_unchecked(&buf[..upto]) }
        }
    };
    valid.graphemes(true).count()
}

pub(crate) fn nth_grapheme_byte_end(buf: &[u8], n: usize) -> Option<usize> {
    use unicode_segmentation::UnicodeSegmentation;
    if n == 0 {
        return Some(0);
    }
    let valid = match std::str::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            let upto = e.valid_up_to();
            // SAFETY: valid_up_to() is by definition the length of the
            // longest valid UTF-8 prefix.
            unsafe { std::str::from_utf8_unchecked(&buf[..upto]) }
        }
    };
    let mut pos = 0usize;
    let mut count = 0usize;
    for g in valid.graphemes(true) {
        pos += g.len();
        count += 1;
        if count == n {
            return Some(pos);
        }
    }
    None
}

/// Completion from an async I/O operation.
pub(crate) struct Completion {
    pub(crate) id: u64,
    pub(crate) result: Result<Value, Value>,
}

impl Completion {
    pub(crate) fn new(id: u64, result: Result<Value, Value>) -> Self {
        Completion { id, result }
    }

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
    fn submit(
        &self,
        request: &IoRequest,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<u64, String>;
    fn poll(&self) -> Vec<Completion>;
    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String>;
    fn cancel(&self, id: u64) -> Result<(), String>;
}

/// Type-erased async I/O backend, stored as `Value::external("io-backend", ...)`.
///
/// The primitives downcast to this type. The trait dispatch handles
/// routing to AsyncBackend, MockBackend, or any future backend.
pub(crate) struct AnyBackend(pub(crate) Box<dyn IoBackend>);
