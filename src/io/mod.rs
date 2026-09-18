//! audited: 2026-09-18
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
pub(crate) mod frame;
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
pub(crate) fn grapheme_count_in_valid_prefix(buf: &[u8], gen: crate::segment::Generation) -> usize {
    let valid = match std::str::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            let upto = e.valid_up_to();
            unsafe { std::str::from_utf8_unchecked(&buf[..upto]) }
        }
    };
    crate::segment::grapheme_count(valid, gen)
}

pub(crate) fn nth_grapheme_byte_end(
    buf: &[u8],
    n: usize,
    gen: crate::segment::Generation,
) -> Option<usize> {
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
    for g in crate::segment::graphemes(valid, gen) {
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

/// Where an io completion builds the answer it could not be handed, and the one
/// reference that region is born with (docs/impl/io-inflight.md).
///
/// A spawn, a `read-all`, a resolution and every error are built when the
/// operation finishes, on the scheduler's side of the park, so no
/// `decref_point` names the region they are born in. That region's birth
/// reference is the completion's, and [`hand_over`](Self::hand_over) is where it
/// goes: the struct `Completion::into_value` builds records a counted edge of its
/// own as it stores the value, so the handover follows that store.
///
/// The region is coined on the first allocation and reused by every one after
/// it, so an answer assembled out of several objects is one region and one
/// reference. A completion whose answer is a value the CALLER allocated — a
/// port, a read's buffer — never allocates here and so coins nothing.
pub(crate) struct Birthplace {
    heap: *mut crate::value::fiberheap::FiberHeap,
    region: Option<crate::hir::region::RuntimeRegion>,
}

impl Birthplace {
    /// A birthplace on `heap`, the requesting instance's own (see
    /// [`completion_heap_ptr`]), holding nothing yet.
    pub(crate) fn on(heap: *mut crate::value::fiberheap::FiberHeap) -> Birthplace {
        Birthplace { heap, region: None }
    }

    /// The allocation capability every value this completion builds goes
    /// through, over the one region this birthplace coins.
    pub(crate) fn alloc(&mut self) -> crate::primitives::ctx::Alloc<'_> {
        // SAFETY (both reborrows): the requesting instance's heap, live for as
        // long as the completion being built is — see `hand_over`.
        let region = match self.region {
            Some(region) => region,
            None => {
                let coined = unsafe { (*completion_heap_ptr(self.heap)).new_runtime_region() };
                self.region = Some(coined);
                coined
            }
        };
        let heap = unsafe { &mut *completion_heap_ptr(self.heap) };
        crate::primitives::ctx::Alloc::with_region(region, heap)
    }

    /// An io-completion error value `{:error :kind :message msg}`, built here
    /// like every other answer a completion has to build itself.
    pub(crate) fn error(&mut self, kind: &str, msg: impl Into<String>) -> Value {
        self.alloc().error(kind, msg)
    }

    /// Let go of the birth reference: whoever took the value has recorded a
    /// count of its own. Taking the region is the receipt, so a second handover
    /// releases nothing, and a birthplace that coined nothing reaches nothing.
    pub(crate) fn hand_over(&mut self) {
        let Some(region) = self.region.take() else {
            return;
        };
        // SAFETY: the store this birthplace coined on. Every route that ends a
        // completion runs while that store is live — a value is built on it, and
        // the teardown discard runs before the store tears its regions down
        // (`FiberHeap::quiesce_io_backends`).
        let heap = unsafe { &mut *self.heap };
        crate::value::arena::decref_region(heap, Some(region));
    }
}

/// Completion from an async I/O operation.
///
/// It carries the [`Birthplace`] it was assembled at, because whatever that
/// birthplace coined is this completion's to hand over
/// (docs/impl/io-inflight.md). A completion whose answer the requesting call
/// pre-allocated coined nothing there, and hands over nothing.
pub(crate) struct Completion {
    pub(crate) id: SubmissionId,
    pub(crate) result: Result<Value, Value>,
    birth: Birthplace,
}

impl Completion {
    pub(crate) fn new(id: SubmissionId, birth: Birthplace, result: Result<Value, Value>) -> Self {
        Completion { id, result, birth }
    }

    /// A successful completion carrying `value`.
    pub(crate) fn ok(id: SubmissionId, birth: Birthplace, value: Value) -> Self {
        Completion::new(id, birth, Ok(value))
    }

    /// A failed completion carrying the Elle error value `error`.
    pub(crate) fn err(id: SubmissionId, birth: Birthplace, error: Value) -> Self {
        Completion::new(id, birth, Err(error))
    }

    /// A failed completion whose error this completion builds. Every error a
    /// completion reports is built rather than handed, so this is the one shape
    /// they all take and the birthplace is always the one that coined it.
    pub(crate) fn failed(
        id: SubmissionId,
        mut birth: Birthplace,
        kind: &str,
        msg: impl Into<String>,
    ) -> Self {
        let error = birth.error(kind, msg);
        Completion::err(id, birth, error)
    }

    /// Nobody will read this completion, so nothing takes over what it built.
    /// The backend's teardown drain is the one caller: it runs while the store
    /// is still there, which is what lets the release name a region at all.
    pub(crate) fn discard(mut self) {
        self.birth.hand_over();
    }

    /// Convert to an Elle struct: {:id n :value v :error nil} or {:id n :value nil :error e}.
    ///
    /// Built through the REAPING CALL's own capability — `io/wait` / `io/reap`
    /// collect these structs into the array they return, and both declare
    /// `RegionEffect::Fresh`, so one region carries the whole result and the
    /// caller's single `DecrefValueRegion` reclaims it (docs/impl/region/ctx.md
    /// § "A helper reached from inside a call allocates through THAT call's
    /// ctx"). The `:value`/`:error` payloads are the exception the edge
    /// accounting exists for: the backend built them off this call, on the
    /// requesting instance's heap (see [`completion_heap_ptr`]), so the struct
    /// records a counted edge to each and the resumed fiber's own reference
    /// carries it past this array's demise.
    ///
    /// That edge is also what the completion hands its own reference over TO,
    /// which is why this consumes the completion and releases afterwards: the
    /// struct counts the payload as it stores it, and the region the completion
    /// built the payload in has a holder from that moment on.
    pub(crate) fn into_value(mut self, ctx: &crate::primitives::ctx::Alloc<'_>) -> Value {
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::keyword("id"), Value::int(self.id.as_u64() as i64));
        match &self.result {
            Ok(v) => {
                fields.insert(TableKey::keyword("value"), *v);
                fields.insert(TableKey::keyword("error"), Value::NIL);
            }
            Err(e) => {
                fields.insert(TableKey::keyword("value"), Value::NIL);
                fields.insert(TableKey::keyword("error"), *e);
            }
        }
        let wrapper = ctx.struct_from(fields);
        self.birth.hand_over();
        wrapper
    }
}

/// Async I/O backend trait.
///
/// Implemented by `AsyncBackend` (real I/O via io_uring or thread pool)
/// and `MockBackend` (in-memory, deterministic).
pub(crate) trait IoBackend {
    /// Submit `request` on behalf of `submitter` — the heap its results are
    /// born on, and the fiber that will read them. See
    /// [`Submitter`](crate::io::pending::Submitter).
    fn submit(
        &self,
        request: &IoRequest,
        submitter: crate::io::pending::Submitter,
    ) -> Result<SubmissionId, String>;
    fn poll(&self) -> Vec<Completion>;
    fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String>;
    fn cancel(&self, id: SubmissionId) -> Result<(), String>;
    /// Cancel and drain every in-flight kernel op, and let go of every region
    /// this backend still holds: the ones its filed operations retain, and the
    /// ones its unreaped completions built. Idempotent and a no-op once nothing
    /// is pending. The default is a no-op for a backend that has neither.
    ///
    /// The backend's own `Drop` runs this, but a heap that STRANDS a backend —
    /// one the program never let go of — must run it BEFORE the region sweep.
    /// `FiberHeap::quiesce_io_backends` is that caller, and the order is the
    /// argument: docs/impl/io-inflight.md § "A hold is let go while its store is
    /// still there". Canonical reference `tests/elle/posix.lisp` under
    /// `--wasm=full`.
    fn quiesce(&self) {}

    /// Background worker operations submitted but not yet reaped — the OS
    /// threads this backend has out. A backend that runs its operations in the
    /// kernel (io_uring) or inline (the mock) has none.
    fn workers(&self) -> usize {
        0
    }
}

/// Type-erased async I/O backend, stored as `Value::external("io-backend", ...)`.
///
/// The primitives downcast to this type. The trait dispatch handles
/// routing to AsyncBackend, MockBackend, or any future backend.
pub(crate) struct AnyBackend(pub(crate) Box<dyn IoBackend>);

#[cfg(test)]
mod tests;
