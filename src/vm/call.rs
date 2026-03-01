//! Call and TailCall instruction handlers.
//!
//! Handles:
//! - Native function calls (routes to signal dispatch in signal.rs)
//! - Closure calls with environment setup
//! - Yield-through-calls (suspended frame chain building)
//! - Tail call optimization
//! - JIT compilation and dispatch

use crate::value::error_val;
use crate::value::{
    SignalBits, SuspendedFrame, SymbolId, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD,
};
// SmallVec was tried here but benchmarks showed no improvement over Vec
// for the common 0-8 arg case. The inline storage (64 bytes) touches a
// full cache line regardless of arg count, and the is-inline branch on
// every push adds overhead that cancels out the allocation savings.
use std::rc::Rc;

use super::core::VM;

use crate::jit::{JitCode, JitCompiler, TAIL_CALL_SENTINEL};

/// Helper: set an error signal on the fiber.
fn set_error(fiber: &mut crate::value::Fiber, kind: &str, msg: impl Into<String>) {
    fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
}

impl VM {
    /// Handle the Call instruction.
    ///
    /// Pops the function and arguments from the stack, calls the function,
    /// and pushes the result. Handles native functions, VM-aware functions,
    /// and closures with proper environment setup.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_call(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        let bc: &[u8] = bytecode;
        let arg_count = self.read_u8(bc, ip) as usize;
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

        self.call_inner(func, args, bytecode, constants, closure_env, ip)
    }

    /// Handle the CallArray instruction.
    ///
    /// Like Call, but instead of reading arg_count from bytecode and popping
    /// individual args, pops an args array and uses its elements as arguments.
    /// Used by splice: the lowerer builds an args array, then CallArray
    /// calls the function with those args.
    ///
    /// Stack: \[func, args_array\] → \[result\]
    pub(super) fn handle_call_array(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArray");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArray");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_tuple() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            self.fiber.stack.push(Value::NIL);
            return None;
        };

        self.call_inner(func, args, bytecode, constants, closure_env, ip)
    }

    /// Shared Call/CallArray logic after argument extraction.
    ///
    /// Dispatches native functions, executes closures with environment setup,
    /// handles yield-through-calls and JIT compilation.
    fn call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(args.as_slice());
            return self.handle_primitive_signal(bits, value, bytecode, constants, closure_env, ip);
        }

        if let Some(closure) = func.as_closure() {
            self.fiber.call_depth += 1;
            if self.fiber.call_depth > 1000 {
                set_error(&mut self.fiber, "error", "Stack overflow");
                return Some(SIG_ERROR);
            }

            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // JIT compilation and dispatch — only for non-suspending closures
            // Suspending closures can never be JIT-compiled, so skip profiling overhead
            if !closure.effect.may_suspend() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    return bits;
                }
            }

            // Build the new environment
            let new_env_rc = match self.build_closure_env(closure, &args) {
                Some(env) => env,
                None => {
                    self.fiber.call_depth -= 1;
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            };

            // Execute the closure, saving/restoring the caller's stack.
            // Essential for fiber/signal propagation and yield-through-nested-calls.
            let (bits, _ip) = self.execute_bytecode_saving_stack(
                &closure.bytecode,
                &closure.constants,
                &new_env_rc,
            );

            self.fiber.call_depth -= 1;

            match bits {
                SIG_OK => {
                    let (_, value) = self.fiber.signal.take().unwrap();
                    self.fiber.stack.push(value);
                }
                SIG_YIELD => {
                    // Yield propagated from a nested call. Two cases:
                    //
                    // 1. yield instruction: suspended frames exist — append the
                    //    caller's frame so resume replays the full call stack.
                    //
                    // 2. fiber/signal: no suspended frames — just propagate the
                    //    signal. The fiber saves its own context for resumption.
                    if let Some(mut frames) = self.fiber.suspended.take() {
                        let (_, value) = self.fiber.signal.take().unwrap();

                        let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                        let caller_frame = SuspendedFrame {
                            bytecode: bytecode.clone(),
                            constants: constants.clone(),
                            env: closure_env.clone(),
                            ip: *ip,
                            stack: caller_stack,
                        };

                        frames.push(caller_frame);
                        self.fiber.signal = Some((SIG_YIELD, value));
                        self.fiber.suspended = Some(frames);
                    }
                    return Some(SIG_YIELD);
                }
                _ => {
                    // Other signal (error, etc.) — propagate to caller.
                    return Some(bits);
                }
            }
            return None;
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        self.fiber.stack.push(Value::NIL);
        None
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
    ) -> Option<SignalBits> {
        let arg_count = self.read_u8(bytecode, ip) as usize;
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

        self.tail_call_inner(func, args)
    }

    /// Handle the TailCallArray instruction.
    ///
    /// Like TailCall, but pops an args array instead of individual args.
    /// Stack: \[func, args_array\] → (sets up pending tail call)
    pub(super) fn handle_tail_call_array(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Option<SignalBits> {
        // Suppress unused warnings — these params match the dispatch signature
        let _ = (ip, bytecode);

        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArray");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArray");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_tuple() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            return Some(SIG_ERROR);
        };

        self.tail_call_inner(func, args)
    }

    /// Shared TailCall/TailCallArray logic after argument extraction.
    ///
    /// Dispatches native functions via tail signal handler, sets up pending
    /// tail call for closures with environment building.
    fn tail_call_inner(&mut self, func: Value, args: Vec<Value>) -> Option<SignalBits> {
        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(&args);
            return Some(self.handle_primitive_signal_tail(bits, value));
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Build proper environment using cached vector
            if !Self::populate_env(
                &mut self.tail_call_env_cache,
                &mut self.fiber,
                closure,
                &args,
            ) {
                return Some(SIG_ERROR);
            }
            let new_env_rc = Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information (Rc clones, not data copies)
            self.pending_tail_call = Some((
                closure.bytecode.clone(),
                closure.constants.clone(),
                new_env_rc,
            ));

            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        Some(SIG_ERROR)
    }

    // ── JIT ─────────────────────────────────────────────────────────

    /// Try JIT compilation/dispatch for a closure call.
    ///
    /// Returns `Some(Option<SignalBits>)` if JIT handled the call (the inner
    /// Option follows handle_call's convention), or `None` to fall through
    /// to the interpreter path. Caller is responsible for decrementing
    /// call_depth on the `Some` path.
    fn try_jit_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<Option<SignalBits>> {
        let bytecode_ptr = closure.bytecode.as_ptr();
        let is_hot = self.record_closure_call(bytecode_ptr);

        // Check if we already have JIT code for this closure
        if let Some(jit_code) = self.jit_cache.get(&bytecode_ptr).cloned() {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        // If hot, attempt JIT compilation
        if is_hot {
            if let Some(ref lir_func) = closure.lir_function {
                // Hoist the SymbolId lookup — needed for both batch and solo paths
                let self_sym = self.find_global_sym_for_bytecode(bytecode_ptr);

                // Try batch compilation first for capture-free functions
                if lir_func.num_captures == 0 {
                    if let Some(result) =
                        self.try_batch_jit(lir_func, bytecode_ptr, closure, args, func, self_sym)
                    {
                        return Some(result);
                    }
                }

                // Solo compilation — pass self_sym for direct self-calls
                match JitCompiler::new() {
                    Ok(compiler) => match compiler.compile(lir_func, self_sym) {
                        Ok(jit_code) => {
                            let jit_code = Rc::new(jit_code);
                            self.jit_cache.insert(bytecode_ptr, jit_code.clone());
                            return Some(self.run_jit(&jit_code, closure, args, func));
                        }
                        Err(e) => match &e {
                            crate::jit::JitError::UnsupportedInstruction(_) => {
                                // MakeClosure and other instructions not yet in JIT.
                                // Fall back to interpreter — the function still works.
                            }
                            _ => {
                                panic!(
                                    "JIT compilation failed for pure function: {}. \
                                     This is a bug — pure functions should be JIT-compilable. \
                                     Error: {}",
                                    closure
                                        .lir_function
                                        .as_ref()
                                        .map(|f| f.name.as_deref().unwrap_or("<anon>"))
                                        .unwrap_or("<no lir>"),
                                    e
                                );
                            }
                        },
                    },
                    Err(e) => {
                        panic!("JIT compiler creation failed: {}. This is a bug.", e);
                    }
                }
            }
        }

        None // Fall through to interpreter
    }

    /// Run JIT-compiled code and handle the result.
    ///
    /// Returns `Option<SignalBits>` following handle_call's convention:
    /// `None` to continue dispatch, `Some(bits)` to return immediately.
    fn run_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<SignalBits> {
        let result = self.call_jit(jit_code, closure, args, func);

        // Check if the JIT function (or a callee) set an error or halt
        if matches!(self.fiber.signal, Some((SIG_ERROR | SIG_HALT, _))) {
            self.fiber.stack.push(Value::NIL);
            return None; // Let the dispatch loop's signal check deal with it
        }

        // Check for pending tail call (JIT function did a TailCall)
        if result.to_bits() == TAIL_CALL_SENTINEL {
            if let Some((tail_bc, tail_consts, tail_env)) = self.pending_tail_call.take() {
                match self.execute_closure_bytecode(&tail_bc, &tail_consts, &tail_env) {
                    Ok(val) => {
                        self.fiber.stack.push(val);
                        return None;
                    }
                    Err(e) => {
                        set_error(&mut self.fiber, "error", e);
                        self.fiber.stack.push(Value::NIL);
                        return None;
                    }
                }
            }
        }

        self.fiber.stack.push(result);
        None
    }

    /// Call a JIT-compiled function.
    ///
    /// # Safety
    /// The JIT code must have been compiled from the same LIR function that
    /// produced the closure's bytecode. The calling convention must match.
    ///
    /// `func_value` is the original Value representing the closure, used for
    /// self-tail-call detection in the JIT code.
    ///
    /// Uses zero-copy pointer casts for both env and args since Value is
    /// `#[repr(transparent)]` over u64.
    fn call_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
        func_value: Value,
    ) -> Value {
        // Zero-copy: Value is #[repr(transparent)] over u64, so &[Value]
        // has the same layout as &[u64]. Cast pointers directly.
        let env_ptr = if closure.env.is_empty() {
            std::ptr::null()
        } else {
            closure.env.as_ptr() as *const u64
        };

        let result_bits = unsafe {
            jit_code.call(
                env_ptr,
                args.as_ptr() as *const u64,
                args.len() as u32,
                self as *mut VM as *mut (),
                func_value.to_bits(),
            )
        };

        unsafe { Value::from_bits(result_bits) }
    }

    // ── Batch JIT ────────────────────────────────────────────────────

    /// Try batch JIT compilation for a hot function and its call peers.
    ///
    /// Returns `Some` if batch compilation succeeded and the hot function
    /// was executed. Returns `None` to fall through to solo compilation.
    fn try_batch_jit(
        &mut self,
        lir_func: &Rc<crate::lir::LirFunction>,
        _bytecode_ptr: *const u8,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
        hot_sym: Option<SymbolId>,
    ) -> Option<Option<SignalBits>> {
        let group = crate::jit::discover_compilation_group(lir_func, &self.globals);
        if group.is_empty() {
            return None;
        }

        let hot_sym = hot_sym?;

        let compiler = match JitCompiler::new() {
            Ok(c) => c,
            Err(e) => {
                panic!("JIT compiler creation failed: {}. This is a bug.", e);
            }
        };

        let mut members = Vec::with_capacity(group.len() + 1);
        members.push(crate::jit::BatchMember {
            sym: hot_sym,
            lir: lir_func,
        });

        for (sym, lir) in &group {
            if *sym != hot_sym {
                members.push(crate::jit::BatchMember { sym: *sym, lir });
            }
        }

        if members.len() <= 1 {
            return None;
        }

        let results = match compiler.compile_batch(&members) {
            Ok(r) => r,
            Err(e) => match &e {
                crate::jit::JitError::UnsupportedInstruction(_) => {
                    // Some member has an instruction the JIT can't handle.
                    // Fall through to solo compilation for the hot function.
                    return None;
                }
                _ => {
                    panic!(
                        "Batch JIT compilation failed: {}. \
                         This is a bug — all members were pre-validated as JIT-compilable.",
                        e
                    );
                }
            },
        };

        // Insert all compiled functions into cache
        let mut hot_jit_code = None;
        for (sym, jit_code) in results {
            let jit_code = Rc::new(jit_code);
            let idx = sym.0 as usize;
            if let Some(val) = self.globals.get(idx) {
                if let Some(peer_closure) = val.as_closure() {
                    let peer_bc_ptr = peer_closure.bytecode.as_ptr();
                    self.jit_cache.insert(peer_bc_ptr, jit_code.clone());
                    if sym == hot_sym {
                        hot_jit_code = Some(jit_code);
                    }
                }
            }
        }

        if let Some(jit_code) = hot_jit_code {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        None
    }

    /// Find the SymbolId for a global closure matching the given bytecode pointer.
    ///
    /// O(n) over globals, but runs at most once per hot function (subsequent
    /// calls hit the jit_cache).
    fn find_global_sym_for_bytecode(&self, bytecode_ptr: *const u8) -> Option<SymbolId> {
        for (i, val) in self.globals.iter().enumerate() {
            if let Some(closure) = val.as_closure() {
                if closure.bytecode.as_ptr() == bytecode_ptr {
                    return Some(SymbolId(i as u32));
                }
            }
        }
        None
    }

    // ── Environment building ────────────────────────────────────────

    /// Build a closure environment from captured variables and arguments.
    ///
    /// Reuses `self.env_cache` to avoid a fresh Vec allocation per call.
    /// Returns `None` if `populate_env` fails (e.g., bad keyword args for `&keys`/`&named`).
    pub(super) fn build_closure_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Rc<Vec<Value>>> {
        if !Self::populate_env(&mut self.env_cache, &mut self.fiber, closure, args) {
            return None;
        }
        Some(Rc::new(self.env_cache.clone()))
    }

    /// Populate an environment buffer with captures, arguments, and local slots.
    ///
    /// Shared by `build_closure_env` (which uses `env_cache`) and
    /// `tail_call_inner` (which uses `tail_call_env_cache`). The two caches
    /// can't alias — a tail call may occur inside a closure call that is
    /// still using `env_cache`.
    ///
    /// Returns `false` if keyword argument collection fails (error set on fiber).
    fn populate_env(
        buf: &mut Vec<Value>,
        fiber: &mut crate::value::Fiber,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> bool {
        buf.clear();
        let needed = closure.env_capacity();
        if buf.capacity() < needed {
            buf.reserve(needed - buf.len());
        }
        buf.extend((*closure.env).iter().cloned());

        match closure.arity {
            crate::value::Arity::AtLeast(min) => {
                // Total fixed slots = num_params - 1 (rest slot is last param)
                let fixed_slots = closure.num_params - 1;

                // Determine how many positional args to consume for fixed slots.
                // For &keys/&named, keyword args should not fill optional slots —
                // once we see a keyword past the required params, the rest are
                // keyword arguments for the collector.
                let collects_keywords = matches!(
                    closure.vararg_kind,
                    crate::hir::VarargKind::Struct | crate::hir::VarargKind::StrictStruct(_)
                );
                let provided_fixed = if collects_keywords {
                    // Always fill required slots, then fill optional slots
                    // only with non-keyword args
                    let mut count = args.len().min(min);
                    while count < fixed_slots && count < args.len() {
                        if args[count].as_keyword_name().is_some() {
                            break;
                        }
                        count += 1;
                    }
                    count
                } else {
                    args.len().min(fixed_slots)
                };

                // Push args for fixed slots (required + optional)
                for (i, arg) in args[..provided_fixed].iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
                // Fill missing optional slots with nil
                for i in provided_fixed..fixed_slots {
                    Self::push_param(buf, closure, i, Value::NIL);
                }

                // Collect remaining args into rest slot
                let rest_args = if args.len() > provided_fixed {
                    &args[provided_fixed..]
                } else {
                    &[]
                };
                let collected = match &closure.vararg_kind {
                    crate::hir::VarargKind::List => Self::args_to_list(rest_args),
                    crate::hir::VarargKind::Struct => {
                        match Self::args_to_struct_static(fiber, rest_args, None) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                    crate::hir::VarargKind::StrictStruct(ref keys) => {
                        match Self::args_to_struct_static(fiber, rest_args, Some(keys)) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                };
                Self::push_param(buf, closure, fixed_slots, collected);
            }
            crate::value::Arity::Range(_, max) => {
                // All slots are fixed (no rest param)
                // Push provided args
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
                // Fill missing optional slots with nil
                for i in args.len()..max {
                    Self::push_param(buf, closure, i, Value::NIL);
                }
            }
            crate::value::Arity::Exact(_) => {
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
            }
        }

        // Add empty LocalCells for locally-defined variables
        let num_locally_defined = closure.num_locals.saturating_sub(closure.num_params);
        for _ in 0..num_locally_defined {
            buf.push(Value::local_cell(Value::NIL));
        }

        true
    }

    /// Push a parameter value into the environment buffer, wrapping in a
    /// LocalCell if the cell_params_mask indicates it's needed.
    #[inline]
    fn push_param(buf: &mut Vec<Value>, closure: &crate::value::Closure, i: usize, val: Value) {
        if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
            buf.push(Value::local_cell(val));
        } else {
            buf.push(val);
        }
    }

    /// Collect values into an Elle list (cons chain terminated by EMPTY_LIST).
    fn args_to_list(args: &[Value]) -> Value {
        let mut list = Value::EMPTY_LIST;
        for arg in args.iter().rev() {
            list = Value::cons(*arg, list);
        }
        list
    }

    /// Collect alternating key-value args into an immutable struct.
    /// Returns `None` if odd number of args or non-keyword keys (error set on fiber).
    /// If `valid_keys` is `Some`, returns error on unknown keys (strict `&named` mode).
    fn args_to_struct_static(
        fiber: &mut crate::value::Fiber,
        args: &[Value],
        valid_keys: Option<&[String]>,
    ) -> Option<Value> {
        use crate::value::types::TableKey;
        use std::collections::BTreeMap;

        if args.is_empty() {
            return Some(Value::struct_from(BTreeMap::new()));
        }

        if !args.len().is_multiple_of(2) {
            set_error(
                fiber,
                "error",
                format!("odd number of keyword arguments ({} args)", args.len()),
            );
            return None;
        }

        let mut map = BTreeMap::new();
        for i in (0..args.len()).step_by(2) {
            let key = match TableKey::from_value(&args[i]) {
                Some(TableKey::Keyword(k)) => k,
                _ => {
                    set_error(
                        fiber,
                        "error",
                        format!(
                            "keyword argument key must be a keyword, got {}",
                            args[i].type_name()
                        ),
                    );
                    return None;
                }
            };

            // Strict validation for &named
            if let Some(valid) = valid_keys {
                if !valid.iter().any(|v| v == &key) {
                    set_error(
                        fiber,
                        "error",
                        format!(
                            "unknown named parameter :{}, valid parameters are: {}",
                            key,
                            valid
                                .iter()
                                .map(|v| format!(":{}", v))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                    return None;
                }
            }

            let table_key = TableKey::Keyword(key.clone());
            if map.contains_key(&table_key) {
                set_error(
                    fiber,
                    "error",
                    format!("duplicate keyword argument :{}", key),
                );
                return None;
            }
            map.insert(table_key, args[i + 1]);
        }
        Some(Value::struct_from(map))
    }
}
