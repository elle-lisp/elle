//! I/O subsystem: request types and backends.
//!
//! `IoBackend` is the async submission-and-completion model: `submit`
//! enqueues a request; `poll`/`wait` harvest completions; `cancel`
//! aborts in-flight work.  Implemented by `AsyncBackend` (io_uring or
//! thread pool) and `MockBackend` (in-memory, deterministic).

pub mod aio;
pub(crate) mod completion;
/// Bridge eventfd helpers — Linux only (the eventfd POLL_ADD that wakes the
/// io_uring wait from an off-ring worker). Also backs `chan`'s Linux wake fd.
#[cfg(target_os = "linux")]
pub(crate) mod eventfd;
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

/// Build an error string of the form `"{context}: {os-error}"` from the
/// current `errno` (via `std::io::Error::last_os_error`). Centralises the
/// `format!("...: {}", std::io::Error::last_os_error())` boilerplate that
/// recurs across the backends.
pub(crate) fn os_error(context: &str) -> String {
    format!("{}: {}", context, std::io::Error::last_os_error())
}

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

/// Identifies an in-flight async I/O submission.
///
/// Minted by a backend's [`IoBackend::submit`] and echoed back on the
/// matching [`Completion`]. Internally a monotonically increasing `u64`;
/// the raw value only escapes at the kernel ABI (io_uring `user_data`),
/// the worker-thread transport, and the Lisp boundary (as an integer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SubmissionId(u64);

impl SubmissionId {
    /// Wrap a raw counter / `user_data` value as a submission id.
    pub(crate) const fn from_raw(raw: u64) -> Self {
        SubmissionId(raw)
    }

    /// The underlying `u64`, for the kernel ABI, worker transport, or
    /// Lisp boundary.
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for SubmissionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The heap an io completion value (result or error) is built on: the requesting
/// instance's own heap, threaded from [`IoBackend::submit`] as `origin_heap` and
/// stored on the backend so every completion built by the scheduler-thread harvest
/// names it explicitly. Every submit path carries a real heap (`io/submit` passes
/// `ctx.heap_mut()`, the WASM host passes its instance heap), so this is the
/// identity that documents the requirement; a null here is a backend that failed
/// to thread its requester's heap.
pub(crate) fn completion_heap_ptr(
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> *mut crate::value::fiberheap::FiberHeap {
    debug_assert!(
        !origin_heap.is_null(),
        "io completion built with a null origin_heap — every backend must carry \
         the requesting instance's heap"
    );
    origin_heap
}

/// An io-completion error value `{:error :kind :message msg}`, born in a fresh
/// region on `origin_heap` (the requesting instance's heap, see
/// [`completion_heap_ptr`]). The io backends build completion errors through a
/// `NativeCtx` over that heap, and the value escapes to the requesting fiber,
/// freed value-based by its `DecrefValueRegion`.
pub(crate) fn io_error(
    kind: &str,
    msg: impl Into<String>,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Value {
    let heap = unsafe { &mut *completion_heap_ptr(origin_heap) };
    let ctx = crate::primitives::ctx::Alloc::new(heap);
    ctx.error(kind, msg)
}

/// Completion from an async I/O operation.
pub(crate) struct Completion {
    pub(crate) id: SubmissionId,
    pub(crate) result: Result<Value, Value>,
}

impl Completion {
    pub(crate) fn new(id: SubmissionId, result: Result<Value, Value>) -> Self {
        Completion { id, result }
    }

    /// A successful completion carrying `value`.
    pub(crate) fn ok(id: SubmissionId, value: Value) -> Self {
        Completion {
            id,
            result: Ok(value),
        }
    }

    /// A failed completion carrying the Elle error value `error`.
    pub(crate) fn err(id: SubmissionId, error: Value) -> Self {
        Completion {
            id,
            result: Err(error),
        }
    }

    /// Convert to an Elle struct: {:id n :value v :error nil} or {:id n :value nil :error e}.
    /// Built on `origin_heap` (the requesting instance's heap; see
    /// [`completion_heap_ptr`]).
    pub(crate) fn to_value(&self, origin_heap: *mut crate::value::fiberheap::FiberHeap) -> Value {
        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("id".into()),
            Value::int(self.id.as_u64() as i64),
        );
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
        let heap = unsafe { &mut *completion_heap_ptr(origin_heap) };
        let ctx = crate::primitives::ctx::Alloc::new(heap);
        ctx.struct_from(fields)
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
    ) -> Result<SubmissionId, String>;
    fn poll(&self) -> Vec<Completion>;
    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String>;
    fn cancel(&self, id: SubmissionId) -> Result<(), String>;
}

/// Type-erased async I/O backend, stored as `Value::external("io-backend", ...)`.
///
/// The primitives downcast to this type. The trait dispatch handles
/// routing to AsyncBackend, MockBackend, or any future backend.
pub(crate) struct AnyBackend(pub(crate) Box<dyn IoBackend>);

#[cfg(test)]
mod tests;
