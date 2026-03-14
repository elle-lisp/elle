//! Primitive signal dispatch.
//!
//! Routes signal bits returned by NativeFn primitives to the appropriate
//! handler: stack push for SIG_OK, error storage for SIG_ERROR, fiber
//! execution for SIG_RESUME/SIG_PROPAGATE/SIG_ABORT, VM state reads
//! for SIG_QUERY.

use crate::value::error_val;
use crate::value::{
    BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ABORT, SIG_ERROR, SIG_HALT, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
};
use std::rc::Rc;

use super::core::VM;

impl VM {
    /// Handle signal bits returned by a primitive in a Call position.
    ///
    /// Returns `None` to continue the dispatch loop, or `Some(bits)` to
    /// return from the dispatch loop (for yields/signals).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_primitive_signal(
        &mut self,
        bits: SignalBits,
        value: Value,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        location_map: &Rc<crate::error::LocationMap>,
    ) -> Option<SignalBits> {
        // Dispatch uses exact equality for VM-internal signals (which are
        // produced by specific primitives with known bit patterns) and
        // contains() for user-facing signals (which can be composed, e.g.
        // SIG_ERROR | SIG_IO from an I/O primitive that errors).

        if bits.is_ok() {
            // SIG_OK — normal return, push result
            self.fiber.stack.push(value);
            return None;
        }

        // --- VM-internal signals (exact match — never composed) ---

        if bits == SIG_RESUME {
            return self.handle_fiber_resume_signal(
                value,
                bytecode,
                constants,
                closure_env,
                ip,
                location_map,
            );
        }

        if bits == SIG_PROPAGATE {
            return self.handle_fiber_propagate_signal(value);
        }

        if bits == SIG_ABORT && value.as_fiber().is_some() {
            return self.handle_fiber_abort_signal(value, bytecode, constants, closure_env, ip);
        }

        if bits == SIG_QUERY {
            // arena/allocs needs mutable VM access to call the thunk —
            // handle before dispatch_query (which takes &self).
            if let Some(cons) = value.as_cons() {
                if cons.first.as_keyword_name() == Some("arena/allocs") {
                    let thunk = cons.rest;
                    match self.handle_arena_allocs(thunk) {
                        Ok(val) => {
                            self.fiber.stack.push(val);
                            return None;
                        }
                        Err(bits) => return Some(bits),
                    }
                }
            }
            let (sig, result) = self.dispatch_query(value);
            if sig == SIG_ERROR {
                self.fiber.signal = Some((SIG_ERROR, result));
                self.fiber.stack.push(Value::NIL);
            } else {
                self.fiber.stack.push(result);
            }
            return None;
        }

        // --- User-facing signals (contains — handles composed bits) ---

        if bits.contains(SIG_ERROR) {
            // Store the error in fiber.signal. The dispatch loop will
            // see it and return the full bits (preserving SIG_IO etc.).
            self.fiber.signal = Some((bits, value));
            self.fiber.stack.push(Value::NIL);
            return None;
        }

        if bits.contains(SIG_HALT) {
            self.fiber.signal = Some((bits, value));
            return Some(bits);
        }

        // Any suspending signal: SIG_YIELD, user-defined (bits 16+),
        // or any combination. All remaining signals after the checks above
        // are suspension signals — save the stack into a SuspendedFrame so
        // call.rs can build the caller frame chain on resume.
        let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
        let frame = SuspendedFrame::Bytecode(BytecodeFrame {
            bytecode: bytecode.clone(),
            constants: constants.clone(),
            env: closure_env.clone(),
            ip: *ip,
            stack: saved_stack,
            active_allocator: crate::value::fiber_heap::save_active_allocator(),
            location_map: location_map.clone(),
        });
        self.fiber.signal = Some((bits, value));
        self.fiber.suspended = Some(vec![frame]);
        Some(bits)
    }

    /// Handle signal bits returned by a primitive in a TailCall position.
    ///
    /// Always returns SignalBits (tail calls always return from the dispatch loop).
    pub(super) fn handle_primitive_signal_tail(
        &mut self,
        bits: SignalBits,
        value: Value,
    ) -> SignalBits {
        // Mirrors handle_primitive_signal but for tail position
        // (always returns SignalBits, never None). Same dispatch
        // strategy: exact match for VM-internal, contains() for
        // user-facing composed signals.

        if bits.is_ok() {
            self.fiber.signal = Some((SIG_OK, value));
            return SIG_OK;
        }

        // --- VM-internal signals (exact match — never composed) ---

        if bits == SIG_RESUME {
            return self.handle_fiber_resume_signal_tail(value);
        }

        if bits == SIG_PROPAGATE {
            return self.handle_fiber_propagate_signal_tail(value);
        }

        if bits == SIG_ABORT && value.as_fiber().is_some() {
            return self.handle_fiber_abort_signal_tail(value);
        }

        if bits == SIG_QUERY {
            // arena/allocs needs mutable VM access to call the thunk —
            // handle before dispatch_query (which takes &self).
            if let Some(cons) = value.as_cons() {
                if cons.first.as_keyword_name() == Some("arena/allocs") {
                    let thunk = cons.rest;
                    match self.handle_arena_allocs(thunk) {
                        Ok(val) => {
                            self.fiber.signal = Some((SIG_OK, val));
                            return SIG_OK;
                        }
                        Err(bits) => return bits,
                    }
                }
            }
            let (sig, result) = self.dispatch_query(value);
            self.fiber.signal = Some((sig, result));
            return sig;
        }

        // --- User-facing signals (contains — handles composed bits) ---

        if bits.contains(SIG_ERROR) {
            self.fiber.signal = Some((bits, value));
            return bits;
        }

        if bits.contains(SIG_HALT) {
            self.fiber.signal = Some((bits, value));
            return bits;
        }

        // --- Suspending and unknown signals ---

        self.fiber.signal = Some((bits, value));
        bits
    }
}

impl VM {
    /// Dispatch a VM state query. Value is (operation . argument).
    ///
    /// The operation can be a keyword or a string. Keywords are resolved
    /// via the content-addressed keyword registry; strings are used
    /// directly. SIG_QUERY is for questions that can only be answered
    /// from the VM's context (call counts, global bindings, current fiber).
    ///
    /// Operations:
    /// - (:"call-count" . closure) — return call count for closure
    /// - (:"doc" . name) — return formatted documentation for a primitive
    /// - (:"global?" . symbol) — return true if symbol is bound as a global
    /// - (:"fiber/self" . _) — return the currently executing fiber, or nil
    /// - (:"list-primitives" . _) — return sorted list of all primitive names
    /// - (:"primitive-meta" . name) — return struct with primitive metadata
    /// - (:"arena/stats" . _) — return struct with heap arena :count and :capacity
    /// - (:"arena/count" . _) — return heap arena object count as int (zero overhead)
    /// - (:"arena/scope-stats" . _) — return scope allocation stats {:enters N :dtors-run N}
    pub(crate) fn dispatch_query(&self, value: Value) -> (SignalBits, Value) {
        let cons = match value.as_cons() {
            Some(c) => c,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "SIG_QUERY: expected cons cell".to_string()),
                );
            }
        };

        // Accept keyword or string as operation identifier.
        let op_name: String = if let Some(name) = cons.first.as_keyword_name() {
            name.to_string()
        } else if let Some(s) = cons.first.with_string(|s| s.to_string()) {
            s
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    "SIG_QUERY: operation must be a keyword or string".to_string(),
                ),
            );
        };
        let arg = cons.rest;

        match op_name.as_str() {
            "call-count" => {
                if let Some(closure) = arg.as_closure() {
                    let ptr = closure.template.bytecode.as_ptr();
                    (SIG_OK, Value::int(self.get_closure_call_count(ptr) as i64))
                } else {
                    (SIG_OK, Value::int(0))
                }
            }
            "global?" => {
                // No global mutable state — always false.
                let _ = arg;
                (SIG_OK, Value::FALSE)
            }
            "doc" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s.to_string()
                } else {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "doc: expected string or keyword".to_string()),
                    );
                };
                // Look up builtin docs by name.
                // TODO(chunk-8): user-defined closure docs are no longer
                // accessible via globals; needs eval-scoped lookup.
                if let Some(doc) = self.docs.get(&name) {
                    (SIG_OK, Value::string(doc.format()))
                } else {
                    (
                        SIG_OK,
                        Value::string(format!("No documentation found for '{}'", name)),
                    )
                }
            }
            "fiber/self" => (SIG_OK, self.current_fiber_value.unwrap_or(Value::NIL)),
            "list-primitives" => {
                // arg is nil (no filter) or a keyword/string category name
                let category_filter: Option<String> = if arg.is_nil() {
                    None
                } else if let Some(k) = arg.as_keyword_name() {
                    Some(k.to_string())
                } else {
                    arg.with_string(|s| s.to_string())
                };

                let mut names: Vec<&String> = if let Some(ref cat) = category_filter {
                    self.docs
                        .iter()
                        .filter(|(_, doc)| doc.category == cat.as_str())
                        .map(|(name, _)| name)
                        .collect()
                } else {
                    self.docs.keys().collect()
                };
                names.sort();
                let values: Vec<Value> =
                    names.iter().map(|n| Value::string(n.to_string())).collect();
                (SIG_OK, crate::value::list(values))
            }
            "primitive-meta" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s.to_string()
                } else {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            "primitive-meta: expected string or keyword".to_string(),
                        ),
                    );
                };
                if let Some(doc) = self.docs.get(&name) {
                    use crate::value::heap::TableKey;
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("name".to_string()),
                        Value::string(doc.name.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("doc".to_string()),
                        Value::string(doc.doc.to_string()),
                    );
                    // params as a list of strings
                    let params: Vec<Value> = doc
                        .params
                        .iter()
                        .map(|p| Value::string(p.to_string()))
                        .collect();
                    fields.insert(
                        TableKey::Keyword("params".to_string()),
                        crate::value::list(params),
                    );
                    fields.insert(
                        TableKey::Keyword("category".to_string()),
                        Value::string(doc.category.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("example".to_string()),
                        Value::string(doc.example.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("arity".to_string()),
                        Value::string(format!("{}", doc.arity)),
                    );
                    fields.insert(
                        TableKey::Keyword("signal".to_string()),
                        Value::string(format!("{}", doc.signal)),
                    );
                    // aliases as a list of strings
                    let aliases: Vec<Value> = doc
                        .aliases
                        .iter()
                        .map(|a| Value::string(a.to_string()))
                        .collect();
                    fields.insert(
                        TableKey::Keyword("aliases".to_string()),
                        crate::value::list(aliases),
                    );
                    (SIG_OK, Value::struct_from(fields))
                } else {
                    (SIG_OK, Value::NIL)
                }
            }
            "arena/stats" => {
                use crate::value::heap::{
                    heap_arena_capacity, heap_arena_len, heap_arena_object_limit, heap_arena_peak,
                    TableKey,
                };
                use std::collections::BTreeMap;
                let mut fields = BTreeMap::new();
                fields.insert(
                    TableKey::Keyword("count".to_string()),
                    Value::int(heap_arena_len() as i64),
                );
                fields.insert(
                    TableKey::Keyword("capacity".to_string()),
                    Value::int(heap_arena_capacity() as i64),
                );
                let limit_val = match heap_arena_object_limit() {
                    Some(n) => Value::int(n as i64),
                    None => Value::NIL,
                };
                fields.insert(TableKey::Keyword("object-limit".to_string()), limit_val);
                // Bytes: estimate from object count × 128 bytes per object
                fields.insert(
                    TableKey::Keyword("bytes".to_string()),
                    Value::int((heap_arena_len() * 128) as i64),
                );
                // Peak object count (high-water mark)
                let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
                let peak = if heap_ptr.is_null() {
                    heap_arena_peak() as i64
                } else {
                    unsafe { (*heap_ptr).peak_alloc_count() as i64 }
                };
                fields.insert(TableKey::Keyword("peak".to_string()), Value::int(peak));
                (SIG_OK, Value::struct_from(fields))
            }
            "arena/scope-stats" => {
                use crate::value::fiber_heap::with_current_heap_mut;
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;
                let (enters, dtors_run) =
                    with_current_heap_mut(|heap| (heap.scope_enters(), heap.scope_dtors_run()))
                        .unwrap_or((0, 0));
                let mut fields = BTreeMap::new();
                fields.insert(
                    TableKey::Keyword("enters".to_string()),
                    Value::int(enters as i64),
                );
                fields.insert(
                    TableKey::Keyword("dtors-run".to_string()),
                    Value::int(dtors_run as i64),
                );
                (SIG_OK, Value::struct_from(fields))
            }
            "arena/fiber-stats" => {
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;
                let fiber_handle = match arg.as_fiber() {
                    Some(h) => h,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "arena/fiber-stats: expected fiber, got {}",
                                    arg.type_name()
                                ),
                            ),
                        );
                    }
                };
                match fiber_handle.try_with(|fiber| {
                    let heap = &fiber.heap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("count".to_string()),
                        Value::int(heap.len() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("bytes".to_string()),
                        Value::int((heap.len() * 128) as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("peak".to_string()),
                        Value::int(heap.peak_alloc_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("scope-enters".to_string()),
                        Value::int(heap.scope_enters() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("dtors-run".to_string()),
                        Value::int(heap.scope_dtors_run() as i64),
                    );
                    Value::struct_from(fields)
                }) {
                    Some(v) => (SIG_OK, v),
                    None => (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "arena/fiber-stats: fiber is currently executing".to_string(),
                        ),
                    ),
                }
            }
            _ => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("SIG_QUERY: unknown operation: {}", op_name),
                ),
            ),
        }
    }
}

impl VM {
    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Uses `execute_bytecode_saving_stack` (re-entrant VM call). The thunk
    /// runs on the current fiber — same heap, same globals, same parameter
    /// frames. Yield from the thunk is propagated upward (not handled here);
    /// callers should only pass non-yielding (inert signal) closures.
    ///
    /// The before/after count snapshots bracket the thunk's execution to
    /// measure net allocations.
    ///
    /// Returns `Ok(cons(result, net_allocs))` on success, or `Err(bits)` on error/halt.
    pub(crate) fn handle_arena_allocs(&mut self, thunk: Value) -> Result<Value, SignalBits> {
        let closure = match thunk.as_closure() {
            Some(c) => c.clone(),
            None => {
                let err = error_val("type-error", "arena/allocs: expected a closure");
                self.fiber.signal = Some((SIG_ERROR, err));
                self.fiber.stack.push(Value::NIL);
                return Err(SIG_ERROR);
            }
        };

        // Snapshot count before
        let before = {
            let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
            if heap_ptr.is_null() {
                crate::value::heap::heap_arena_len()
            } else {
                unsafe { (*heap_ptr).len() }
            }
        };

        // Build a proper env (captures + local slots) for the thunk.
        // Passing closure.env directly would omit local variable slots,
        // causing StoreUpvalue panics for closures with locals.
        let thunk_env = self
            .build_closure_env(&closure, &[])
            .expect("arena/allocs: zero-arg thunk env build cannot fail");

        // Execute the thunk via execute_bytecode_saving_stack
        let exec_result = self.execute_bytecode_saving_stack(
            &closure.template.bytecode,
            &closure.template.constants,
            &thunk_env,
            &closure.template.location_map,
        );

        if exec_result.bits.contains(SIG_ERROR) {
            // Propagate the error — signal is already set by the inner execution
            return Err(exec_result.bits);
        }

        // Get result from signal
        let result = self
            .fiber
            .signal
            .take()
            .map(|(_, v)| v)
            .unwrap_or(Value::NIL);

        // Snapshot count after
        let after = {
            let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
            if heap_ptr.is_null() {
                crate::value::heap::heap_arena_len()
            } else {
                unsafe { (*heap_ptr).len() }
            }
        };

        let net = (after as i64) - (before as i64);
        Ok(Value::cons(result, Value::int(net)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocationMap;
    use crate::value::{SIG_DEBUG, SIG_HALT, SIG_IO, SIG_YIELD};

    /// Create minimal test fixtures for handle_primitive_signal.
    type TestFixtures = (Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>, Rc<LocationMap>);
    fn test_fixtures() -> TestFixtures {
        (
            Rc::new(vec![]),
            Rc::new(vec![]),
            Rc::new(vec![]),
            Rc::new(LocationMap::new()),
        )
    }

    // -- handle_primitive_signal (Call position) --

    #[test]
    fn composed_error_io_treated_as_error() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_ERROR | SIG_IO;

        let result = vm.handle_primitive_signal(
            bits,
            Value::string("boom"),
            &bc,
            &consts,
            &env,
            &mut ip,
            &loc,
        );

        // SIG_ERROR handler returns None (continue dispatch loop)
        assert!(
            result.is_none(),
            "SIG_ERROR|SIG_IO should return None (error path)"
        );
        // Error stored in fiber.signal with full composed bits
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR), "signal should contain SIG_ERROR");
        assert!(sig.contains(SIG_IO), "signal should preserve SIG_IO bit");
        // NIL pushed to stack (error convention)
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
        // The value is preserved
        assert_eq!(val.with_string(|s| s.to_string()), Some("boom".to_string()));
    }

    #[test]
    fn composed_yield_io_creates_suspended_frame() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_YIELD | SIG_IO;

        let result =
            vm.handle_primitive_signal(bits, Value::int(42), &bc, &consts, &env, &mut ip, &loc);

        // Should return Some(bits) to exit dispatch loop
        assert_eq!(result, Some(SIG_YIELD | SIG_IO));
        // Should create a suspended frame
        assert!(
            vm.fiber.suspended.is_some(),
            "should create suspended frame"
        );
        let frames = vm.fiber.suspended.take().unwrap();
        assert_eq!(frames.len(), 1);
        // Signal stored with full composed bits
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_YIELD | SIG_IO);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn bare_io_creates_suspended_frame() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;

        let result =
            vm.handle_primitive_signal(SIG_IO, Value::int(99), &bc, &consts, &env, &mut ip, &loc);

        // SIG_IO alone should also create a suspended frame
        assert_eq!(result, Some(SIG_IO));
        assert!(
            vm.fiber.suspended.is_some(),
            "SIG_IO alone should create suspended frame"
        );
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_IO);
    }

    #[test]
    fn sig_ok_pushes_value() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;

        let result =
            vm.handle_primitive_signal(SIG_OK, Value::int(7), &bc, &consts, &env, &mut ip, &loc);

        assert!(result.is_none());
        assert_eq!(vm.fiber.stack.pop(), Some(Value::int(7)));
        assert!(vm.fiber.signal.is_none());
    }

    #[test]
    fn bare_error_stores_signal() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;

        let result = vm.handle_primitive_signal(
            SIG_ERROR,
            Value::string("err"),
            &bc,
            &consts,
            &env,
            &mut ip,
            &loc,
        );

        assert!(result.is_none());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_ERROR);
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
    }

    #[test]
    fn halt_returns_immediately() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;

        let result =
            vm.handle_primitive_signal(SIG_HALT, Value::int(0), &bc, &consts, &env, &mut ip, &loc);

        assert_eq!(result, Some(SIG_HALT));
    }

    #[test]
    fn error_takes_priority_over_yield() {
        // SIG_ERROR | SIG_YIELD should be handled as error (higher priority)
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_ERROR | SIG_YIELD;

        let result = vm.handle_primitive_signal(
            bits,
            Value::string("err"),
            &bc,
            &consts,
            &env,
            &mut ip,
            &loc,
        );

        // Error path returns None
        assert!(result.is_none());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR));
        // NIL pushed (error convention)
        assert_eq!(vm.fiber.stack.pop(), Some(Value::NIL));
        // No suspended frame created
        assert!(vm.fiber.suspended.is_none());
    }

    #[test]
    fn unknown_signal_propagates() {
        let mut vm = VM::new();
        let (bc, consts, env, loc) = test_fixtures();
        let mut ip = 0usize;
        let bits = SIG_DEBUG; // not handled by any specific branch

        let result =
            vm.handle_primitive_signal(bits, Value::int(1), &bc, &consts, &env, &mut ip, &loc);

        assert_eq!(result, Some(SIG_DEBUG));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_DEBUG);
    }

    // -- handle_primitive_signal_tail (TailCall position) --

    #[test]
    fn tail_composed_error_io_treated_as_error() {
        let mut vm = VM::new();
        let bits = SIG_ERROR | SIG_IO;

        let result = vm.handle_primitive_signal_tail(bits, Value::string("boom"));

        // Should return the full composed bits
        assert!(result.contains(SIG_ERROR));
        assert!(result.contains(SIG_IO));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR));
        assert!(sig.contains(SIG_IO));
    }

    #[test]
    fn tail_composed_yield_io_propagates() {
        let mut vm = VM::new();
        let bits = SIG_YIELD | SIG_IO;

        let result = vm.handle_primitive_signal_tail(bits, Value::int(42));

        assert_eq!(result, SIG_YIELD | SIG_IO);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_YIELD | SIG_IO);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn tail_sig_ok_stores_ok() {
        let mut vm = VM::new();

        let result = vm.handle_primitive_signal_tail(SIG_OK, Value::int(5));

        assert_eq!(result, SIG_OK);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(5));
    }

    #[test]
    fn tail_error_priority_over_yield() {
        let mut vm = VM::new();
        let bits = SIG_ERROR | SIG_YIELD;

        let result = vm.handle_primitive_signal_tail(bits, Value::string("err"));

        assert!(result.contains(SIG_ERROR));
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR));
    }
}
