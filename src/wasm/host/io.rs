//! audited: 2026-09-20
//! I/O a compiled module asks for at the top level, with no scheduler to take
//! it: the backend it reaches and the answer it reads back.
//!
//! src/wasm/AGENTS.md
//! docs/impl/io-inflight.md

use super::ElleHost;
use crate::io::request::IoRequest;
use crate::io::{AnyBackend, Completion};
use crate::signals::SIG_IO;
use crate::value::fiber::SignalBits;
use crate::value::Value;

impl ElleHost {
    /// Handle SIG_IO from a primitive call.
    ///
    /// When inside a fiber (fiber_id_stack is non-empty), propagate
    /// SIG_IO so the scheduler can drive I/O through the event loop.
    /// Otherwise, execute I/O inline via the bound backend or SyncBackend.
    pub fn maybe_execute_io(&mut self, bits: SignalBits, value: Value) -> (SignalBits, Value) {
        if bits.raw() & SIG_IO.raw() == 0 {
            return (bits, value);
        }

        // Inside a fiber: propagate SIG_IO to the scheduler
        if !self.fiber_id_stack.is_empty() {
            return (bits, value);
        }

        // Top-level: execute I/O inline
        let request = match value.as_external::<IoRequest>() {
            Some(r) => r,
            None => return (bits, value),
        };

        if let Some(backend_val) = self.find_io_backend() {
            if let Some(async_be) = backend_val.as_external::<AnyBackend>() {
                if let Ok(_id) = async_be.0.submit(
                    request,
                    crate::io::pending::Submitter::detached(self.heap_ptr()),
                ) {
                    if let Ok(completions) = async_be.0.wait(-1) {
                        if let Some(answer) = inline_answer(completions) {
                            return answer;
                        }
                    }
                }
            }
        }
        // Fallback: use the lazily-initialized backend
        self.execute_io_inline(request)
    }

    /// Execute an I/O request using the lazily-initialized backend.
    pub(crate) fn execute_io_inline(&mut self, request: &IoRequest) -> (SignalBits, Value) {
        let backend = match &self.io_backend {
            Some(_) => self.io_backend.as_ref().unwrap(),
            None => match crate::io::aio::AsyncBackend::new_with_unicode(
                unsafe { &*self.vm }.unicode_generation(),
                // This backend serves inline I/O for a WASM module rather than
                // a scheduler a program parameterized, so nothing named a
                // keepalive for it: the default stands.
                None,
            ) {
                Ok(be) => {
                    self.io_backend = Some(AnyBackend(Box::new(be)));
                    self.io_backend.as_ref().unwrap()
                }
                Err(e) => {
                    let heap = unsafe { &mut *self.heap_ptr() };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    return (
                        crate::value::fiber::SIG_ERROR,
                        ctx.error("io-error", format!("failed to create I/O backend: {}", e)),
                    );
                }
            },
        };
        if let Ok(_id) = backend.0.submit(
            request,
            crate::io::pending::Submitter::detached(self.heap_ptr()),
        ) {
            if let Ok(completions) = backend.0.wait(-1) {
                if let Some(answer) = inline_answer(completions) {
                    return answer;
                }
            }
        }
        let heap = unsafe { &mut *self.heap_ptr() };
        let ctx = crate::primitives::ctx::Alloc::new(heap);
        (
            crate::value::fiber::SIG_ERROR,
            ctx.error("io-error", "I/O submission failed"),
        )
    }

    /// Search param_frames for a value that is an I/O backend.
    fn find_io_backend(&self) -> Option<Value> {
        for frame in self.param_frames.iter().rev() {
            for &(_, value) in frame {
                if value.as_external::<AnyBackend>().is_some() {
                    return Some(value);
                }
            }
        }
        None
    }
}

/// The signal and value one inline wait answers with, or `None` when the wait
/// returned nothing.
///
/// The answer comes out of the first completion, and takes the reference to
/// whatever that completion built along with it: this tier reclaims no region
/// while it runs, so there is nothing here to hand that reference to
/// (docs/impl/io-inflight.md). The rest of the wait's completions have no
/// reader at all, so each is discarded rather than let go.
fn inline_answer(completions: Vec<Completion>) -> Option<(SignalBits, Value)> {
    let mut completions = completions.into_iter();
    let answered = completions.next()?;
    Completion::discard_all(completions);
    Some(match answered.into_result() {
        Ok(v) => (crate::value::fiber::SIG_OK, v),
        Err(e) => (crate::value::fiber::SIG_ERROR, e),
    })
}
