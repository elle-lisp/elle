// audited: 2026-10-21
//! `arena/allocs`: the net allocations of a thunk run to completion on the current fiber.
//!
//! docs/impl/vm.md

use super::*;

impl VM {
    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Runs the thunk through [`VM::run_thunk_to_completion`], the re-entrant
    /// VM call that drives the `fiber/resume` `SIG_SWITCH` trampoline. So a
    /// thunk that spawns and resumes fibers is measured to completion. The
    /// resume's allocations fall between the two snapshots, and
    /// `(result . net)` carries the thunk's real result, not the resumed
    /// child's value (tests/impl/arena.lisp, and the `fiber-spawn-10`
    /// scenario in tests/impl/resource.lisp). The thunk must still be
    /// non-*yielding*: it must not suspend its own caller. Returns
    /// `(SIG_OK, pair(result, net))` on success, or `(SIG_ERROR, err)` / the
    /// propagated signal on failure.
    pub(super) fn handle_arena_allocs(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        thunk: Value,
    ) -> (SignalBits, Value) {
        let closure = match thunk.as_closure() {
            Some(c) => c.clone(),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "arena/allocs: expected a closure"),
                );
            }
        };

        let before = unsafe { (*self.heap_ptr).visible_len() };

        let thunk_env = self
            .build_closure_env(&closure, &[])
            .expect("arena/allocs: zero-arg thunk env build cannot fail");

        // Hand the thunk its executing-closure register via the one-shot — the
        // measured-thunk entry runs a closure body like any other entrant.
        self.pending_entry_closure = thunk;
        let depth = self.fiber.param_depth();
        let bits = self.run_thunk_to_completion(&closure.template.code(), &thunk_env);

        if !bits.is_empty() {
            // Propagate the error/signal — fiber.signal is already set by inner
            // execution. A suspend-class park leaves as a value, abandoned with
            // its host. An error's parked chain ends here, and the error leaves
            // as this call's own.
            self.abandon_hosted_park(bits);
            self.discard_hosted_error(bits, depth);
            let (sig, val) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            return (sig, val);
        }

        let result = self
            .fiber
            .signal
            .take()
            .map(|(_, v)| v)
            .unwrap_or(Value::NIL);

        let after = unsafe { (*self.heap_ptr).visible_len() };

        let net = (after as i64) - (before as i64);
        let answer = ctx.pair(result, Value::int(net));
        // The thunk's result left its compiled body through the return
        // convention carrying one owed reference, and this boundary consumes it.
        // The pair, fresh in the call's own region, is the value the caller
        // releases, not the result inside it. The pair's alloc-time scan already
        // counted the embedding. So consuming the mint after the pair is built
        // leaves the result held by the pair and freed by its cascade (the
        // `allocs-result` row of tests/impl/probe/native.lisp).
        let heap = unsafe { &mut *self.heap_ptr };
        let region = crate::value::arena::region_of(heap, result);
        if let Some(region) = region {
            heap.decref_region_if_present(region);
        }
        (SIG_OK, answer)
    }
}
