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

        etrace!(
            self,
            crate::config::trace_bits::SIGNAL,
            "signal",
            "bits={} value_type={}",
            bits,
            value.type_name()
        );

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
            // Mutable queries — handled before dispatch_query (which takes &self).
            if let Some(cons) = value.as_cons() {
                if cons.first.as_keyword_name().as_deref() == Some("arena/allocs") {
                    let thunk = cons.rest;
                    match self.handle_arena_allocs(thunk) {
                        Ok(val) => {
                            self.fiber.stack.push(val);
                            return None;
                        }
                        Err(bits) => return Some(bits),
                    }
                }
                if cons.first.as_keyword_name().as_deref() == Some("vm/config-set") {
                    let result = self.handle_vm_config_set(cons.rest);
                    self.fiber.stack.push(result);
                    return None;
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
            location_map: location_map.clone(),
            // Caller frame for a suspending primitive: on resume, the primitive's
            // eventual return value flows as current_value and must be pushed as
            // the result of the Call instruction.
            push_resume_value: true,
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
            // Mutable queries — handled before dispatch_query (which takes &self).
            if let Some(cons) = value.as_cons() {
                if cons.first.as_keyword_name().as_deref() == Some("arena/allocs") {
                    let thunk = cons.rest;
                    match self.handle_arena_allocs(thunk) {
                        Ok(val) => {
                            self.fiber.signal = Some((SIG_OK, val));
                            return SIG_OK;
                        }
                        Err(bits) => return bits,
                    }
                }
                if cons.first.as_keyword_name().as_deref() == Some("vm/config-set") {
                    let result = self.handle_vm_config_set(cons.rest);
                    self.fiber.signal = Some((SIG_OK, result));
                    return SIG_OK;
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

    // ── Capability denial ─────────────────────────────────────────────

    /// Handle capability denial in Call position.
    ///
    /// The fiber tried to call a primitive whose signal bits overlap with
    /// the fiber's `withheld` capabilities. Instead of running the primitive,
    /// emit a signal with the blocked bits and a denial payload struct.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_capability_denial(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        location_map: &Rc<crate::error::LocationMap>,
    ) -> Option<SignalBits> {
        let payload = Self::build_denial_payload(def, blocked, args);

        // Save the stack and build a suspended frame (same as suspending signals)
        let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
        let frame = SuspendedFrame::Bytecode(BytecodeFrame {
            bytecode: bytecode.clone(),
            constants: constants.clone(),
            env: closure_env.clone(),
            ip: *ip,
            stack: saved_stack,
            location_map: location_map.clone(),
            push_resume_value: true,
        });
        self.fiber.signal = Some((blocked, payload));
        self.fiber.suspended = Some(vec![frame]);
        Some(blocked)
    }

    /// Handle capability denial in TailCall position.
    pub(super) fn handle_capability_denial_tail(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
    ) -> SignalBits {
        let payload = Self::build_denial_payload(def, blocked, args);
        self.fiber.signal = Some((blocked, payload));
        blocked
    }

    /// Build the denial payload struct.
    ///
    /// Returns `{:error :capability-denied :denied <keyword-set>
    ///           :primitive <name> :func <native-fn> :args <array>}`.
    fn build_denial_payload(
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
    ) -> Value {
        use crate::value::heap::TableKey;
        use std::collections::BTreeMap;

        let registry = crate::signals::registry::global_registry().lock().unwrap();
        let denied_keywords = registry.bits_to_keywords(blocked);

        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("error".into()),
            Value::keyword("capability-denied"),
        );
        fields.insert(
            TableKey::Keyword("denied".into()),
            Value::set(denied_keywords.into_iter().collect()),
        );
        fields.insert(
            TableKey::Keyword("primitive".into()),
            Value::string(def.name),
        );
        fields.insert(TableKey::Keyword("func".into()), Value::native_fn(def));
        fields.insert(
            TableKey::Keyword("args".into()),
            Value::array(args.to_vec()),
        );

        Value::struct_from(fields)
    }
}

impl VM {
    /// Dispatch a VM state query. Value is (operation . argument).
    ///
    /// The operation can be a keyword or a string. Keywords are resolved
    /// via the content-addressed keyword registry; strings are used
    /// directly. SIG_QUERY is for questions that can only be answered
    /// from the VM's context (call counts, documentation, current fiber).
    ///
    /// Operations:
    /// - (:"call-count" . closure) — return call count for closure
    /// - (:"doc" . name) — return formatted documentation for a primitive
    /// - (:"global?" . symbol) — always false (no runtime globals exist)
    /// - (:"fiber/self" . _) — return the currently executing fiber, or nil
    /// - (:"list-primitives" . _) — return sorted list of all primitive names
    /// - (:"primitive-meta" . name) — return struct with primitive metadata
    /// - (:"arena/stats" . nil) — return unified stats struct (12 fields) for current fiber
    /// - (:"arena/stats" . fiber) — return unified stats struct for a suspended/dead fiber
    /// - (:"arena/count" . _) — return heap arena object count as int (zero overhead)
    /// - (:"jit?" . closure) — true if closure has JIT-compiled native code
    pub(crate) fn dispatch_query(&mut self, value: Value) -> (SignalBits, Value) {
        let cons = match value.as_cons() {
            Some(c) => c,
            None => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "SIG_QUERY: expected cons cell".to_string()),
                );
            }
        };

        // Accept keyword or string as operation identifier.
        let op_name: String = if let Some(name) = cons.first.as_keyword_name() {
            name
        } else if let Some(s) = cons.first.with_string(|s| s.to_string()) {
            s
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
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
                    s
                } else {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "doc: expected string or keyword".to_string()),
                    );
                };
                // Look up builtin docs by name. Stdlib closures are handled
                // upstream: the analyzer passes them through as closure values,
                // and prim_doc extracts the docstring from closure.template.doc
                // before the SIG_QUERY reaches here. This path is only reached
                // for native primitives, special forms, and explicit string args.
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
            "fiber/caps" => {
                let caps = crate::signals::CAP_MASK.subtract(self.fiber.withheld);
                let registry = crate::signals::registry::global_registry().lock().unwrap();
                let keywords = registry.bits_to_keywords(caps);
                (SIG_OK, Value::set(keywords.into_iter().collect()))
            }
            "list-primitives" => {
                // arg is nil (no filter) or a keyword/string category name
                let category_filter: Option<String> = if arg.is_nil() {
                    None
                } else if let Some(k) = arg.as_keyword_name() {
                    Some(k)
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
                let values: Vec<Value> = unsafe {
                    let symbols_ptr = crate::context::get_symbol_table();
                    names
                        .iter()
                        .map(|n| {
                            if let Some(ptr) = symbols_ptr {
                                let id = (*ptr).intern(n);
                                Value::symbol(id.0)
                            } else {
                                Value::string(n)
                            }
                        })
                        .collect()
                };
                (SIG_OK, crate::value::list(values))
            }
            "primitive-meta" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s
                } else if let Some(sym_id) = arg.as_symbol() {
                    match crate::context::resolve_symbol_name(sym_id) {
                        Some(s) => s,
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "internal-error",
                                    format!(
                                        "primitive-meta: symbol ID {} not found in symbol table",
                                        sym_id
                                    ),
                                ),
                            );
                        }
                    }
                } else {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            "primitive-meta: expected string, keyword, or symbol".to_string(),
                        ),
                    );
                };
                if let Some(doc) = self.docs.get(&name) {
                    use crate::value::heap::TableKey;
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("name".to_string()),
                        Value::string(doc.name),
                    );
                    fields.insert(TableKey::Keyword("doc".to_string()), Value::string(doc.doc));
                    // params as a list of strings
                    let params: Vec<Value> = doc.params.iter().map(|p| Value::string(*p)).collect();
                    fields.insert(
                        TableKey::Keyword("params".to_string()),
                        crate::value::list(params),
                    );
                    fields.insert(
                        TableKey::Keyword("category".to_string()),
                        Value::string(doc.category),
                    );
                    fields.insert(
                        TableKey::Keyword("example".to_string()),
                        Value::string(doc.example),
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
                    let aliases: Vec<Value> =
                        doc.aliases.iter().map(|a| Value::string(*a)).collect();
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
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;

                /// Build the unified stats struct from a FiberHeap reference.
                /// Fields: :object-count, :peak-count, :allocated-bytes, :object-limit,
                /// :scope-depth, :dtor-count, :root-live-count, :root-alloc-count,
                /// :shared-count, :active-allocator, :scope-enter-count, :scope-dtor-count.
                fn build_stats(heap: &crate::value::FiberHeap) -> Value {
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("object-count".to_string()),
                        Value::int(heap.visible_len() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("peak-count".to_string()),
                        Value::int(heap.peak_alloc_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("allocated-bytes".to_string()),
                        Value::int(heap.allocated_bytes() as i64),
                    );
                    let limit_val = match heap.object_limit() {
                        Some(n) => Value::int(n as i64),
                        None => Value::NIL,
                    };
                    fields.insert(TableKey::Keyword("object-limit".to_string()), limit_val);
                    fields.insert(
                        TableKey::Keyword("scope-depth".to_string()),
                        Value::int(heap.scope_depth() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("dtor-count".to_string()),
                        Value::int(heap.dtor_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("root-live-count".to_string()),
                        Value::int(heap.root_live() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("root-alloc-count".to_string()),
                        Value::int(heap.root_alloc_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("shared-count".to_string()),
                        Value::int(heap.shared_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("active-allocator".to_string()),
                        Value::keyword("slab"),
                    );
                    fields.insert(
                        TableKey::Keyword("scope-enter-count".to_string()),
                        Value::int(heap.scope_enters() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("scope-dtor-count".to_string()),
                        Value::int(heap.scope_dtors_run() as i64),
                    );
                    Value::struct_from(fields)
                }

                if arg.is_nil() {
                    // 0-arg path: read from the current fiber's heap.
                    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
                    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
                    let stats = unsafe { build_stats(&*heap_ptr) };
                    (SIG_OK, stats)
                } else {
                    // 1-arg path: read from the provided fiber's heap.
                    let fiber_handle = match arg.as_fiber() {
                        Some(h) => h,
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!("arena/stats: expected fiber, got {}", arg.type_name()),
                                ),
                            );
                        }
                    };
                    match fiber_handle.try_with(|fiber| build_stats(&fiber.heap)) {
                        Some(v) => (SIG_OK, v),
                        None => (
                            SIG_ERROR,
                            error_val(
                                "state-error",
                                "arena/stats: fiber is currently executing".to_string(),
                            ),
                        ),
                    }
                }
            }
            "jit/rejections" => {
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;

                // Sort by call count ascending (coldest first, hottest last).
                let mut entries: Vec<_> = self.jit_rejections.iter().collect();
                entries.sort_by_key(|(ptr, _)| {
                    self.closure_call_counts.get(ptr).copied().unwrap_or(0)
                });

                let structs: Vec<Value> = entries
                    .into_iter()
                    .map(|(ptr, info)| {
                        let mut fields = BTreeMap::new();
                        let name = info.name.as_deref().unwrap_or("<anon>");
                        fields.insert(TableKey::Keyword("name".to_string()), Value::string(name));
                        fields.insert(
                            TableKey::Keyword("reason".to_string()),
                            Value::string(info.reason.to_string()),
                        );
                        let calls = self.closure_call_counts.get(ptr).copied().unwrap_or(0);
                        fields.insert(
                            TableKey::Keyword("calls".to_string()),
                            Value::int(calls as i64),
                        );
                        Value::struct_from(fields)
                    })
                    .collect();
                (SIG_OK, crate::value::list(structs))
            }
            "jit?" => {
                if let Some(closure) = arg.as_closure() {
                    let ptr = closure.template.bytecode.as_ptr();
                    (SIG_OK, Value::bool(self.jit_cache.contains_key(&ptr)))
                } else {
                    (SIG_OK, Value::FALSE)
                }
            }
            "vm/config" => self.dispatch_vm_config_read(arg),
            #[cfg(feature = "mlir")]
            "mlir/compile-spirv" => {
                // arg is (closure . workgroup-size)
                let (closure_val, wg_size): (Value, u32) = match arg.as_cons() {
                    Some(c) => (c.first, c.rest.as_int().unwrap_or(256) as u32),
                    None => (arg, 256),
                };

                let closure = match closure_val.as_closure() {
                    Some(c) => c,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "mlir/compile-spirv: expected closure, got {}",
                                    closure_val.type_name()
                                ),
                            ),
                        )
                    }
                };
                let lir = match &closure.template.lir_function {
                    Some(lir) => lir,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "mlir-error",
                                "mlir/compile-spirv: closure has no LIR".to_string(),
                            ),
                        )
                    }
                };
                if !lir.is_gpu_eligible() {
                    return (
                        SIG_ERROR,
                        error_val(
                            "mlir-error",
                            "mlir/compile-spirv: closure is not GPU-eligible".to_string(),
                        ),
                    );
                }
                let key = closure.template.bytecode.as_ptr();
                let cache = self
                    .mlir_cache
                    .get_or_insert_with(crate::mlir::MlirCache::new);
                match cache.compile_spirv(key, lir, wg_size) {
                    Ok(bytes) => (SIG_OK, Value::bytes(bytes.to_vec())),
                    Err(e) => (
                        SIG_ERROR,
                        error_val("mlir-error", format!("mlir/compile-spirv: {}", e)),
                    ),
                }
            }
            #[cfg(feature = "mlir")]
            "git" => {
                // arg is (closure . workgroup-size)
                let (closure_val, wg_size): (Value, u32) = match arg.as_cons() {
                    Some(c) => (c.first, c.rest.as_int().unwrap_or(256) as u32),
                    None => (arg, 256),
                };

                let closure = match closure_val.as_closure() {
                    Some(c) => c,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!("git: expected closure, got {}", closure_val.type_name()),
                            ),
                        )
                    }
                };
                // Already cached? Return early.
                if closure.template.spirv.get().is_some() {
                    return (SIG_OK, closure_val);
                }
                let lir = match &closure.template.lir_function {
                    Some(lir) => lir,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val("mlir-error", "git: closure has no LIR".to_string()),
                        )
                    }
                };
                if !lir.is_gpu_eligible() {
                    return (
                        SIG_ERROR,
                        error_val("mlir-error", "git: closure is not GPU-eligible".to_string()),
                    );
                }
                let key = closure.template.bytecode.as_ptr();
                let cache = self
                    .mlir_cache
                    .get_or_insert_with(crate::mlir::MlirCache::new);
                match cache.compile_spirv(key, lir, wg_size) {
                    Ok(bytes) => {
                        // Cache on the template (OnceCell — idempotent).
                        let _ = closure.template.spirv.set(bytes.to_vec());
                        (SIG_OK, closure_val)
                    }
                    Err(e) => (SIG_ERROR, error_val("mlir-error", format!("git: {}", e))),
                }
            }
            _ => (
                SIG_ERROR,
                error_val(
                    "argument-error",
                    format!("SIG_QUERY: unknown operation: {}", op_name),
                ),
            ),
        }
    }

    /// Handle `(vm/config)` read — returns config struct or specific field.
    fn dispatch_vm_config_read(&self, arg: Value) -> (SignalBits, Value) {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        let rc = &self.runtime_config;

        if arg.is_nil() {
            // Full config struct
            let mut map = BTreeMap::new();
            map.insert(
                TableKey::from_value(&Value::keyword("jit")).unwrap(),
                Value::keyword(rc.jit.keyword()),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("wasm")).unwrap(),
                Value::keyword(rc.wasm.keyword()),
            );
            // trace as a set of keywords
            let trace_set: Vec<Value> = rc.trace.iter().map(|k| Value::keyword(k)).collect();
            map.insert(
                TableKey::from_value(&Value::keyword("trace")).unwrap(),
                Value::set(trace_set.into_iter().collect()),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("stats")).unwrap(),
                Value::bool(rc.stats),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("debug-bytecode")).unwrap(),
                Value::bool(rc.debug_bytecode),
            );
            (SIG_OK, Value::struct_from(map))
        } else if let Some(kw) = arg.as_keyword_name() {
            match kw.as_str() {
                "jit" => (SIG_OK, Value::keyword(rc.jit.keyword())),
                "wasm" => (SIG_OK, Value::keyword(rc.wasm.keyword())),
                "trace" => {
                    let trace_set: Vec<Value> =
                        rc.trace.iter().map(|k| Value::keyword(k)).collect();
                    (SIG_OK, Value::set(trace_set.into_iter().collect()))
                }
                "stats" => (SIG_OK, Value::bool(rc.stats)),
                _ => (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("vm/config: unknown field :{}", kw),
                    ),
                ),
            }
        } else {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "vm/config: expected keyword or nil, got {}",
                        arg.type_name()
                    ),
                ),
            )
        }
    }

    /// Handle `(vm/config-set key value)` — mutates the VM's RuntimeConfig.
    fn handle_vm_config_set(&mut self, arg: Value) -> Value {
        let cons = match arg.as_cons() {
            Some(c) => c,
            None => return error_val("type-error", "vm/config-set: expected (key . value)"),
        };
        let key = cons.first;
        let val = cons.rest;

        let kw = match key.as_keyword_name() {
            Some(k) => k,
            None => {
                return error_val(
                    "type-error",
                    format!(
                        "vm/config-set: key must be a keyword, got {}",
                        key.type_name()
                    ),
                )
            }
        };

        match kw.as_str() {
            "jit" => {
                if let Some(closure) = val.as_closure() {
                    // Custom policy via closure — store on VM (future: store the closure)
                    let _ = closure; // TODO: store for actual dispatch
                    self.runtime_config.jit = crate::config::JitPolicy::Custom;
                    self.jit_enabled = true;
                    self.jit_hotness_threshold = 0;
                } else if let Some(policy_kw) = val.as_keyword_name() {
                    match crate::config::JitPolicy::from_keyword(&policy_kw) {
                        Some(policy) => {
                            self.jit_enabled = policy.enabled();
                            self.jit_hotness_threshold = policy.threshold();
                            self.runtime_config.jit = policy;
                        }
                        None => {
                            return error_val(
                                "argument-error",
                                format!("vm/config-set :jit: unknown policy :{}", policy_kw),
                            )
                        }
                    }
                } else {
                    return error_val(
                        "type-error",
                        format!(
                            "vm/config-set :jit: expected keyword or closure, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "wasm" => {
                if let Some(policy_kw) = val.as_keyword_name() {
                    match crate::config::WasmPolicy::from_keyword(&policy_kw) {
                        Some(policy) => {
                            self.runtime_config.wasm = policy;
                        }
                        None => {
                            return error_val(
                                "argument-error",
                                format!("vm/config-set :wasm: unknown policy :{}", policy_kw),
                            )
                        }
                    }
                } else {
                    return error_val(
                        "type-error",
                        format!(
                            "vm/config-set :wasm: expected keyword, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "trace" => {
                // Accept a set of keywords
                if let Some(set) = val.as_set() {
                    let mut keywords = std::collections::HashSet::new();
                    for v in set.iter() {
                        if let Some(k) = v.as_keyword_name() {
                            keywords.insert(k);
                        }
                    }
                    self.runtime_config.set_trace(keywords);
                } else {
                    return error_val(
                        "type-error",
                        format!(
                            "vm/config-set :trace: expected set, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "stats" => {
                self.runtime_config.stats = val.is_truthy();
                Value::NIL
            }
            _ => error_val(
                "argument-error",
                format!("vm/config-set: unknown field :{}", kw),
            ),
        }
    }
}

impl VM {
    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Uses `execute_bytecode_saving_stack` (re-entrant VM call). The thunk
    /// runs on the current fiber — same heap, same parameter
    /// frames. Yield from the thunk is propagated upward (not handled here);
    /// callers should only pass non-yielding (silent signal) closures.
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

        // Snapshot count before (visible_len includes shared_alloc)
        let before = {
            let heap_ptr = crate::value::fiberheap::current_heap_ptr();
            debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
            unsafe { (*heap_ptr).visible_len() }
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

        // Snapshot count after (visible_len includes shared_alloc)
        let after = {
            let heap_ptr = crate::value::fiberheap::current_heap_ptr();
            unsafe { (*heap_ptr).visible_len() }
        };

        let net = (after as i64) - (before as i64);
        Ok(Value::cons(result, Value::int(net)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocationMap;
    use crate::value::{SIG_DEBUG, SIG_IO, SIG_YIELD};

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
