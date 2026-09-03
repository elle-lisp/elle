//! Call and TailCall instruction handlers.
//!
//! Handles:
//! - Native function calls (routes to signal dispatch in signal.rs)
//! - Closure calls with environment setup
//! - Yield-through-calls (suspended frame chain building)
//! - Tail call optimization
//!
//! Environment building (closure env population, parameter binding) lives in `env.rs`.

use crate::hir::region::StaticRegion;
use crate::primitives::access::resolve_index;
use crate::value::fiber::{CallFrame, MAX_CALL_DEPTH};
use crate::value::{
    sorted_struct_get, BytecodeFrame, SignalBits, SuspendedFrame, TableKey, Value, SIG_ERROR,
    SIG_FUEL, SIG_HALT, SIG_OK,
};
// SmallVec was tried here but benchmarks showed no improvement over Vec
// for the common 0-8 arg case. The inline storage (64 bytes) touches a
// full cache line regardless of arg count, and the is-inline branch on
// every push adds overhead that cancels out the allocation savings.
use std::rc::Rc;

use super::core::VM;

mod inner;

impl VM {
    /// Handle the Call instruction.
    ///
    /// Pops the function and arguments from the stack, calls the function,
    /// and pushes the result. Handles native functions, VM-aware functions,
    /// and closures with proper environment setup.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_call(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        checked: bool,
    ) -> Option<SignalBits> {
        let bc: &[u8] = code.bytecode();
        let arg_count = self.read_u16(bc, ip) as usize;
        let region_id = self.read_static_region(bc, ip);
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on Call");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on Call"),
            );
        }
        args.reverse();

        self.call_inner(
            func,
            args,
            code,
            closure_env,
            ip,
            instr_ip,
            checked,
            region_id,
        )
    }

    /// Handle the CallArrayMut instruction.
    ///
    /// Like Call, but instead of reading arg_count from bytecode and popping
    /// individual args, pops an args array and uses its elements as arguments.
    /// Used by splice: the lowerer builds an args array, then CallArrayMut
    /// calls the function with those args.
    ///
    /// Stack: \[func, args_array\] → \[result\]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_call_array(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        checked: bool,
    ) -> Option<SignalBits> {
        // Both region operands are decoded first, so `ip` is past them however
        // this handler leaves — the type-error arm below returns without calling.
        let bc: &[u8] = code.bytecode();
        let region_id = self.read_static_region(bc, ip);
        let args_region = self.read_static_region(bc, ip);
        // Claimed before the callee can park: a suspending callee snapshots this
        // activation's region map, and the array's entry must not travel into it.
        let args_array = self.take_splice_args(args_region);

        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            self.set_error(
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            self.release_splice_args(args_array);
            self.fiber.stack.push(Value::NIL);
            return None;
        };

        let bits = self.call_inner(
            func,
            args,
            code,
            closure_env,
            ip,
            instr_ip,
            checked,
            region_id,
        );
        // The callee holds its own reference to every argument by now — the env's
        // `CallArgument` incref for a closure, the pass-through retain for a
        // native — so the array's counted edges are surplus and this reclaim
        // balances the pushes that built it.
        self.release_splice_args(args_array);
        bits
    }

    /// Handle the TailCall instruction.
    ///
    /// Similar to Call but sets up a pending tail call instead of recursing,
    /// enabling tail call optimization.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_tail_call(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
        checked: bool,
    ) -> Option<SignalBits> {
        let arg_count = self.read_u16(bytecode, ip) as usize;
        let region_id = self.read_static_region(bytecode, ip);
        let defer_callee_release = self.read_u8(bytecode, ip) != 0;
        // Closure-cycle merged-arena release slot: `0` encodes `None` (a
        // `StaticRegion` is `NonZeroU32`, so a real slot is never 0). See
        // `LirInstr::TailCall::deferred_release_slot`.
        let deferred_release_slot = StaticRegion::new(self.read_u32(bytecode, ip));
        // The borrowed-argument stash slots. Decoded unconditionally so `ip`
        // stays aligned; a SIGNAL exit consumes their retains
        // (docs/impl/region/mechanism.md § "What the fall-through owes, a signal
        // exit owes too") and the normal fall-through ignores them — it runs the
        // block's own `DecrefValueRegion`s.
        let borrowed_count = self.read_u8(bytecode, ip) as usize;
        let mut borrowed_arg_slots = Vec::with_capacity(borrowed_count);
        for _ in 0..borrowed_count {
            borrowed_arg_slots.push(self.read_u16(bytecode, ip));
        }
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCall");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on TailCall"),
            );
        }
        args.reverse();

        self.tail_call_inner(
            func,
            args,
            checked,
            region_id,
            defer_callee_release,
            deferred_release_slot,
            &borrowed_arg_slots,
            false,
        )
    }

    /// Handle the TailCallArrayMut instruction.
    ///
    /// Like TailCall, but pops an args array instead of individual args.
    /// Stack: \[func, args_array\] → (sets up pending tail call)
    pub(super) fn handle_tail_call_array(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
        checked: bool,
    ) -> Option<SignalBits> {
        let region_id = self.read_static_region(bytecode, ip);
        let args_region = self.read_static_region(bytecode, ip);
        // Claimed before the callee can park, as in the Call position above.
        let args_array = self.take_splice_args(args_region);

        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            self.set_error(
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            self.release_splice_args(args_array);
            return Some(SIG_ERROR);
        };

        // Splice/apply tail call (`TailCallArrayMut`): the closure-callee deferred
        // release and the closure-cycle merged-arena release slot are not wired through this
        // path yet — `false`/`None` keep today's behaviour (no regression). The
        // common `(f …)` tail call uses `TailCall`, which carries both — and the
        // borrowed-argument stash list with them.
        let bits = self.tail_call_inner(func, args, checked, region_id, false, None, &[], true);
        // The array's reclaim, on the one path every outcome of this call passes
        // through: a frame-replacing closure callee never arrives at the block
        // after this instruction, so no emitted release could reach it. The
        // callee has minted its own reference to every argument by now
        // (`own_params`, from the `true` above), so the cascade here balances the
        // pushes that built the array and nothing more.
        self.release_splice_args(args_array);
        bits
    }

    /// Dispatch a collection-as-function call-index (`(arr i)` / `(m :k)` /
    /// `(str i)` / …) with the same per-execution region routing and pass-through
    /// retain a native primitive gets via [`VM::dispatch_native_call`].
    ///
    /// A collection-call is morally a native: it returns either a value it
    /// allocated fresh (a `(str i)` single-grapheme string), which lands in this
    /// call's minted `alloc_region`, or a co-located / stored element it borrows
    /// from the collection's region (an immutable array/struct element shares the
    /// container's region pages; a mutable container's stored value has its own
    /// region kept alive only by the container's stored reference). The borrowed
    /// case is a Rule-5 native-result pass-through (docs/impl/region/rules.md): without an
    /// incref the caller's `DecrefValueRegion` cascade-frees the element under its
    /// consumer's borrow — the call-index UAF family (region-array-element-uaf,
    /// region-struct-call-index-uaf, region-mut-collection-call-index-uaf). The
    /// fresh case lives in `alloc_region` and is skipped — its alloc incref
    /// (rc=1) is already the caller's single owning reference, exactly as in
    /// `dispatch_native_call`. An immediate result (`(b i)` int, `(s v)` bool, a
    /// `nil` default) has no region, so the incref no-ops.
    ///
    /// Shared verbatim by the interpreter (`call_inner` / `tail_call_inner`) and
    /// the JIT (`elle_jit_call` / `elle_jit_tail_call`) so both tiers account
    /// identically. Returns `None` when `func` is not a callable collection (the
    /// caller falls through to its "cannot call" error).
    pub(crate) fn dispatch_collection_call(
        &mut self,
        func: &Value,
        args: &[Value],
        region_id: StaticRegion,
    ) -> Option<Result<Value, (&'static str, String)>> {
        let mint = self.new_runtime_region_for_call_slot(region_id);
        let alloc_region = mint.region();
        let result = {
            // The ctx is the explicit capability `call_collection` allocates
            // through, minted from this call's fresh region exactly as
            // `dispatch_native_call`.
            let mut ctx = crate::primitives::ctx::Alloc::with_region(alloc_region, unsafe {
                &mut *self.heap_ptr
            });
            call_collection(func, args, self.unicode_generation, &mut ctx)
        };
        if let Some(Ok(value)) = result.as_ref() {
            let heap = unsafe { &mut *self.heap_ptr };
            let result_region = crate::value::arena::region_of(heap, *value);
            if result_region != Some(alloc_region) {
                crate::value::arena::incref_for_escape(
                    heap,
                    result_region,
                    crate::value::arena::EscapeSite::CollectionCallResult,
                );
            }
        }
        // A borrowed element or an immediate result left this call's region
        // unallocated, so its id goes back to the free list — the same close-out
        // `dispatch_native_call` makes.
        self.release_unused_call_region(mint);
        result
    }

    /// Call a compiled closure `Value` with the given argument values.
    ///
    /// Used by macro expansion to invoke cached transformer closures without
    /// going through the full `eval_syntax` pipeline, and by trait-method
    /// dispatch. Takes the closure as a `Value` (not a `&Closure`) so the entry
    /// can hand the body its executing-closure register — a self-recursive
    /// transformer or trait method resolves its self-reference to it.
    ///
    /// Returns the closure's return value on success. Returns `Err` when the
    /// value is not a closure, on arity mismatch, error signal, or halt.
    ///
    /// Callers must not pass closures that may yield (signal includes
    /// `SIG_YIELD`). Macro transformer closures are always silent.
    pub fn call_closure(&mut self, closure_val: Value, args: &[Value]) -> Result<Value, String> {
        let Some(closure) = closure_val.as_closure() else {
            return Err(format!(
                "call_closure: not a closure: {}",
                closure_val.type_name()
            ));
        };
        // Arity check — sets fiber.signal on mismatch.
        if !self.check_arity(&closure.template.arity(), args.len()) {
            let (_, err) = self.fiber.signal.take().unwrap();
            return Err(self.format_error_with_location(err));
        }

        // Build the closure environment (captures + param slots + local slots).
        let new_env = match self.build_closure_env(closure, args) {
            Some(env) => env,
            None => {
                let (_, err) = self.fiber.signal.take().unwrap();
                return Err(self.format_error_with_location(err));
            }
        };

        // Execute the closure bytecode, saving/restoring the caller's stack.
        // The one-shot hands the body its executing-closure register.
        self.pending_entry_closure = closure_val;
        let result = self.execute_bytecode_saving_stack(&closure.template.code(), &new_env);

        let bits = result.bits;
        if bits.is_empty() {
            let (_, value) = self.fiber.signal.take().unwrap();
            Ok(value)
        } else if bits == crate::value::SIG_HALT {
            let (_, value) = self.fiber.signal.take().unwrap();
            if value == Value::NIL {
                Ok(value)
            } else {
                Err(self.format_error_with_location(value))
            }
        } else if bits.intersects(crate::value::SIG_ERROR) {
            let (_, err) = self
                .fiber
                .signal
                .take()
                .unwrap_or((crate::value::SIG_ERROR, Value::NIL));
            Err(self.format_error_with_location(err))
        } else {
            // Unexpected suspending signal (yield from macro body — not supported).
            self.fiber.signal.take();
            Err(format!(
                "Unexpected signal from macro transformer: {}",
                crate::signals::registry::format_bits(bits)
            ))
        }
    }
}

mod collection;
pub(crate) use collection::call_collection;

#[cfg(test)]
mod tests;
