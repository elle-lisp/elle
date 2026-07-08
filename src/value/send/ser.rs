use super::syntax::syntax_to_send;
use super::*;

/// Send a traits field. Default traitsets (from the registry) are
/// skipped (sent as NIL) since the receiving thread has its own registry.
/// User-attached traits are deep-copied normally.
fn send_traits(traits: Value, tag: HeapTag, ctx: &mut SerContext<'_>) -> Result<SendValue, String> {
    if traits.is_nil() {
        return Ok(SendValue::Immediate(Value::NIL));
    }
    // Check pointer identity against the registry default for this tag, on the
    // SENDER's heap (the value being serialized lives there). This distinguishes
    // registry defaults (skip) from user-attached @struct traits (send faithfully).
    let default = crate::primitives::traitregistry::default_traits_for(ctx.heap, tag);
    if !default.is_nil() && traits.payload == default.payload {
        return Ok(SendValue::Immediate(Value::NIL));
    }
    // User-attached traits — send normally
    from_value_inner(traits, ctx)
}

/// Per-call serialization context for `SendBundle::from_value`.
pub(super) struct SerContext<'s> {
    /// Intern table being built; read back by callers after serialization.
    pub(super) closures: Vec<SendableClosure>,
    /// Maps `value.payload` (heap pointer address) → intern table index.
    /// Inserted BEFORE recursing into a closure's fields, so back-references find it.
    visited: HashMap<u64, usize>,
    /// The SENDER's symbol table — used to resolve a symbol value's id to its name
    /// so it crosses the thread boundary by name (ids are per-table). Threaded
    /// explicitly (docs/impl/region/ctx.md § "Symbols through the ctx").
    pub(super) symbols: &'s crate::symbol::SymbolTable,
    /// The SENDER's heap — `send_traits` reads its default-traits table to skip
    /// registry-default traitsets (the receiver rebuilds them). The value being
    /// serialized lives on this heap, so it is the right table to compare against.
    pub(super) heap: &'s crate::value::fiberheap::FiberHeap,
}

impl<'s> SerContext<'s> {
    pub(super) fn new(
        heap: &'s crate::value::fiberheap::FiberHeap,
        symbols: &'s crate::symbol::SymbolTable,
    ) -> Self {
        SerContext {
            visited: HashMap::new(),
            closures: Vec::new(),
            symbols,
            heap,
        }
    }
}

/// Make a LIR function shippable across a thread boundary, returning the
/// compound-value pool (or `None` if the LIR must be dropped).
///
/// Two passes:
///   1. Lift each *compound* `ValueConst` operand (quoted list, struct, array,
///      …) into a serialized `lir_value_pool` entry and rewrite the instruction
///      to `Const(ValueRef(idx))`. Serialization goes through `ctx`, so any
///      closures nested in the compound intern into the bundle correctly.
///   2. Delegate to `LirFunction::convert_value_consts_for_send`, which inlines
///      scalar operands and rewrites closure operands to `ClosureRef` (keeping
///      the `lir/closure-value-const-count` accounting). It returns `false` only
///      when a closure operand isn't in the intern table; we then drop the LIR
///      and the worker falls back to bytecode.
///
/// `patch_lir_value_refs` / `patch_lir_closure_refs` invert this on receipt.
fn convert_lir_for_send(
    lir: &mut crate::lir::LirFunction,
    ctx: &mut SerContext<'_>,
) -> Result<Option<Vec<SendValue>>, String> {
    use crate::lir::{value_to_lir_const, LirConst, LirInstr};

    // Pass 1: compound ValueConsts → ValueRef into the pool.
    let mut pool: Vec<SendValue> = Vec::new();
    for block in &mut lir.blocks {
        for si in &mut block.instructions {
            let (dst, value) = match &si.instr {
                LirInstr::ValueConst { dst, value } => (*dst, *value),
                _ => continue,
            };
            // Leave scalars, closures, and native fns for pass 2 / as-is.
            if value.is_native_fn() || value.is_closure() || value_to_lir_const(value).is_some() {
                continue;
            }
            let sv = from_value_inner(value, ctx)?;
            let idx = pool.len();
            pool.push(sv);
            si.instr = LirInstr::Const {
                dst,
                value: LirConst::ValueRef(idx),
            };
        }
    }

    // Pass 2: scalars inline + closures → ClosureRef (or signal a drop).
    if lir.convert_value_consts_for_send(&ctx.visited) {
        Ok(Some(pool))
    } else {
        Ok(None)
    }
}

/// Serialize a closure **template** blueprint (`ClosureTemplate.child_protos`
/// entry) into a `SendableClosure`. Templates have no heap identity to intern,
/// so they are emitted inline; `env`/`squelch_mask` are empty (a blueprint is a
/// pure template). Recurses on the template's own `child_protos` so a worker
/// rebuilds the full nested-lambda tree and every `MakeClosure` resolves.
fn sendable_from_template(
    t: &crate::value::ClosureTemplate,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    let constants: Vec<SendValue> = t
        .constants
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;

    let doc = t.doc.as_deref().map(str::to_string);

    let (lir_function, lir_value_pool) = match t.lir_function.as_ref() {
        Some(lir) => {
            let mut lir = (**lir).clone();
            lir.doc = None;
            lir.syntax = None;
            match convert_lir_for_send(&mut lir, ctx)? {
                Some(pool) => (Some(lir), pool),
                None => (None, Vec::new()),
            }
        }
        None => (None, Vec::new()),
    };

    let child_protos: Vec<SendableClosure> = t
        .child_protos
        .iter()
        .map(|p| sendable_from_template(p, ctx))
        .collect::<Result<_, _>>()?;

    Ok(SendableClosure {
        bytecode: (*t.bytecode).clone(),
        arity: t.arity,
        num_locals: t.num_locals,
        num_captures: t.num_captures,
        num_params: t.num_params,
        constants,
        signal: t.signal,
        capture_params_mask: t.capture_params_mask,
        capture_locals_mask: t.capture_locals_mask.clone(),
        symbol_names: (*t.symbol_names).clone(),
        location_map: (*t.location_map).clone(),
        doc,
        vararg_kind: t.vararg_kind.clone(),
        name: t.name.as_ref().map(|s| s.to_string()),
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir_function,
        lir_value_pool,
        child_protos,
        merged_slots: t.merged_slots.iter().copied().collect(),
    })
}

/// Recursive worker for serialization. Threads SerContext through all recursive calls.
pub(super) fn from_value_inner(
    value: Value,
    ctx: &mut SerContext<'_>,
) -> Result<SendValue, String> {
    // Keywords carry their name for cross-thread re-interning
    if let Some(name) = value.as_keyword_name() {
        return Ok(SendValue::Keyword(name));
    }

    // Symbols carry their name for cross-thread re-interning (IDs are
    // per-table). If the id is not in the sender's table (should not happen),
    // fall through to Immediate.
    if let Some(id) = value.as_symbol() {
        if let Some(name) = ctx.symbols.name(crate::value::SymbolId(id)) {
            return Ok(SendValue::Symbol {
                name: name.to_string(),
                id,
            });
        }
        return Ok(SendValue::Immediate(value));
    }

    // Immediate values are always safe
    if value.is_nil() || value.is_bool() || value.is_int() || value.is_float() {
        return Ok(SendValue::Immediate(value));
    }

    // String values (SSO or heap)
    if let Some(s) = value.with_string(|s| s.to_string()) {
        return Ok(SendValue::String(s));
    }

    // Heap values need deep copying
    if !value.is_heap() {
        return Ok(SendValue::Immediate(value));
    }

    match unsafe { deref(value) } {
        // Strings are immutable and safe
        HeapObject::LString { s, .. } => Ok(SendValue::String(unsafe {
            std::str::from_utf8_unchecked(s.as_slice()).to_string()
        })),

        // Pair cells - deep copy both first and rest, plus traits
        HeapObject::Pair(pair) => {
            let first = from_value_inner(pair.first, ctx)?;
            let rest = from_value_inner(pair.rest, ctx)?;
            let traits = send_traits(pair.traits, HeapTag::Pair, ctx)?;
            Ok(SendValue::Pair(
                Box::new(first),
                Box::new(rest),
                Box::new(traits),
            ))
        }

        // Arrays - deep copy all elements, plus traits
        HeapObject::LArrayMut {
            data: vec_ref,
            traits,
            ..
        } => {
            let borrowed = vec_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow array for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = send_traits(*traits, HeapTag::LArrayMut, ctx)?;
            Ok(SendValue::Array(copied?, Box::new(traits_sv)))
        }

        // Structs - deep copy all values, plus traits
        HeapObject::LStruct {
            data: s, traits, ..
        } => {
            let mut copied = BTreeMap::new();
            for (k, v) in s.iter() {
                if !k.is_sendable() {
                    return Err("Cannot send struct with identity keys".to_string());
                }
                copied.insert(k.clone(), from_value_inner(*v, ctx)?);
            }
            let traits_sv = send_traits(*traits, HeapTag::LStruct, ctx)?;
            Ok(SendValue::Struct(copied, Box::new(traits_sv)))
        }

        // Arrays (immutable) - deep copy all elements, plus traits
        HeapObject::LArray {
            elements: elems,
            traits,
            ..
        } => {
            let copied: Result<Vec<SendValue>, String> =
                elems.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = send_traits(*traits, HeapTag::LArray, ctx)?;
            Ok(SendValue::Tuple(copied?, Box::new(traits_sv)))
        }

        // @string - deep copy the bytes, plus traits
        HeapObject::LStringMut {
            data: buf_ref,
            traits,
            ..
        } => {
            let borrowed = buf_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @string for sending".to_string())?;
            let traits_sv = send_traits(*traits, HeapTag::LStringMut, ctx)?;
            Ok(SendValue::Buffer(borrowed.clone(), Box::new(traits_sv)))
        }

        // User boxes - deep copy the contents if sendable, plus traits
        HeapObject::LBox {
            cell: cell_ref,
            traits,
            ..
        } => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow box for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            let traits_sv = send_traits(*traits, HeapTag::LBox, ctx)?;
            Ok(SendValue::LBox(Box::new(contents), Box::new(traits_sv)))
        }

        // Compiler capture cells - deep copy the contents if sendable, plus traits
        HeapObject::CaptureCell {
            cell: cell_ref,
            traits,
            ..
        } => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow capture cell for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            let traits_sv = send_traits(*traits, HeapTag::CaptureCell, ctx)?;
            Ok(SendValue::CaptureCell(
                Box::new(contents),
                Box::new(traits_sv),
            ))
        }

        // Float values that couldn't be stored inline
        HeapObject::Float(f) => Ok(SendValue::Float(*f)),

        // Mutable @structs — deep copy all values, plus traits
        HeapObject::LStructMut {
            data: map_ref,
            traits,
            ..
        } => {
            let borrowed = map_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @struct for sending".to_string())?;
            let mut copied = BTreeMap::new();
            for (k, v) in borrowed.iter() {
                if !k.is_sendable() {
                    return Err("Cannot send @struct with identity keys".to_string());
                }
                copied.insert(k.clone(), from_value_inner(*v, ctx)?);
            }
            let traits_sv = send_traits(*traits, HeapTag::LStructMut, ctx)?;
            Ok(SendValue::StructMut(copied, Box::new(traits_sv)))
        }

        // Closures: intern into the table, with cycle detection via pre-insertion
        HeapObject::Closure {
            closure: closure_rc,
            traits: _,
        } => {
            // Use value.payload as identity key — for heap values, payload IS the pointer.
            let key = value.payload;

            // Already visited → return Ref to existing intern entry.
            if let Some(&idx) = ctx.visited.get(&key) {
                return Ok(SendValue::Ref(idx));
            }

            // Reserve an index BEFORE recursing so back-references resolve to this entry.
            let idx = ctx.closures.len();
            // Push a placeholder (will be overwritten below).
            ctx.closures.push(SendableClosure {
                bytecode: Vec::new(),
                arity: closure_rc.template.arity,
                num_locals: 0,
                num_captures: 0,
                num_params: 0,
                constants: Vec::new(),
                signal: closure_rc.template.signal,
                capture_params_mask: 0,
                capture_locals_mask: crate::value::CaptureMask::empty(),
                symbol_names: HashMap::new(),
                location_map: LocationMap::new(),
                doc: None,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: None,
                squelch_mask: SignalBits::EMPTY,
                env: Vec::new(),
                lir_function: None,
                lir_value_pool: Vec::new(),
                child_protos: Vec::new(),
                merged_slots: Vec::new(), // placeholder; replaced below
            });
            ctx.visited.insert(key, idx);

            // Serialize environment (may contain back-references to this closure via LBox).
            let env: Result<Vec<SendValue>, String> = closure_rc
                .env
                .iter()
                .map(|v| from_value_inner(*v, ctx))
                .collect();
            let env = env?;

            // Serialize constants.
            let constants: Result<Vec<SendValue>, String> = closure_rc
                .template
                .constants
                .iter()
                .map(|v| from_value_inner(*v, ctx))
                .collect();
            let constants = constants?;

            // Serialize doc (optional) — plain string data, not a heap Value.
            let doc = closure_rc.template.doc.as_deref().map(str::to_string);

            // Clone LIR for JIT in spawned threads. Strip doc (Value/Rc) and
            // syntax (Rc<Syntax>), then convert every cross-thread-unsafe
            // ValueConst: scalars inline, closures → ClosureRef, compounds →
            // ValueRef into `lir_value_pool` (serialized through `ctx` so nested
            // closures intern correctly). The LIR is preserved unconditionally —
            // a spawned closure keeps its JIT-able body across the boundary.
            let (lir_function, lir_value_pool) = match closure_rc.template.lir_function.as_ref() {
                Some(lir) => {
                    let mut lir = (**lir).clone();
                    lir.doc = None;
                    lir.syntax = None;
                    match convert_lir_for_send(&mut lir, ctx)? {
                        Some(pool) => (Some(lir), pool),
                        // A closure-valued ValueConst couldn't be interned — drop
                        // the LIR (the closure still runs via bytecode in the worker).
                        None => (None, Vec::new()),
                    }
                }
                None => (None, Vec::new()),
            };

            // Serialize the nested-lambda blueprints so the worker's reconstructed
            // template carries them and `MakeClosure` resolves by index.
            let child_protos: Vec<SendableClosure> = closure_rc
                .template
                .child_protos
                .iter()
                .map(|p| sendable_from_template(p, ctx))
                .collect::<Result<_, _>>()?;

            // Replace placeholder with complete entry.
            ctx.closures[idx] = SendableClosure {
                bytecode: (*closure_rc.template.bytecode).clone(),
                arity: closure_rc.template.arity,
                num_locals: closure_rc.template.num_locals,
                num_captures: closure_rc.template.num_captures,
                num_params: closure_rc.template.num_params,
                constants,
                signal: closure_rc.template.signal,
                capture_params_mask: closure_rc.template.capture_params_mask,
                capture_locals_mask: closure_rc.template.capture_locals_mask.clone(),

                symbol_names: (*closure_rc.template.symbol_names).clone(),
                location_map: (*closure_rc.template.location_map).clone(),
                doc,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: closure_rc.template.name.as_ref().map(|s| s.to_string()),
                squelch_mask: closure_rc.squelch_mask,
                env,
                lir_function,
                lir_value_pool,
                child_protos,
                merged_slots: closure_rc.template.merged_slots.iter().copied().collect(),
            };

            Ok(SendValue::Ref(idx))
        }

        // (Native-fns are immediates — `Value{TAG_NATIVE_FN, prim_id}` — and
        // serialize via the `Immediate` arm above. The prim_id is stable across
        // threads/processes via deterministic registration, so it re-resolves to
        // the same primitive on the receiver. They never reach this heap match.)

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),

        // Unsafe: thread handles
        HeapObject::ThreadHandle { .. } => Err("Cannot send thread handle".to_string()),

        // Unsafe: fibers (contain execution state with closures)
        HeapObject::Fiber { .. } => Err("Cannot send fiber".to_string()),

        // Parsed syntax: serialized to a self-contained Send-safe mirror.
        HeapObject::Syntax { syntax, .. } => {
            Ok(SendValue::Syntax(Box::new(syntax_to_send(syntax)?)))
        }

        // Unsafe: FFI signatures (contain non-Send types like Cif)
        HeapObject::FFISignature(_, _) => Err("Cannot send FFI signature".to_string()),

        // Unsafe: managed pointers (lifecycle state is not thread-safe with Cell)
        HeapObject::ManagedPointer { .. } => Err("Cannot send managed pointer".to_string()),

        // External objects: channels and stdio ports are sendable, others not.
        HeapObject::External { obj, .. } => match obj.type_name {
            "chan/sender" => crate::primitives::chan::clone_sender(&value)
                .map(|(tx, wake)| SendValue::ChanSender(tx, wake))
                .ok_or_else(|| "Cannot send closed channel sender".to_string()),
            "chan/receiver" => crate::primitives::chan::clone_receiver(&value)
                .map(|(rx, wake)| SendValue::ChanReceiver(rx, wake))
                .ok_or_else(|| "Cannot send closed channel receiver".to_string()),
            // Stdin/Stdout/Stderr ports carry no owned fd — reconstruct fresh in
            // the worker. File/socket ports own their fd and are not sendable.
            "port" => {
                use crate::port::{Port, PortKind};
                match value.as_external::<Port>().map(|p| p.kind()) {
                    Some(k @ (PortKind::Stdin | PortKind::Stdout | PortKind::Stderr)) => {
                        Ok(SendValue::StdioPort(k))
                    }
                    Some(_) => Err(
                        "Cannot send a file or socket port (only stdin/stdout/stderr)".to_string(),
                    ),
                    None => Err("Cannot send port: not a port object".to_string()),
                }
            }
            _ => Err(format!("Cannot send external object: {}", obj.type_name)),
        },

        // Parameters: sendable iff their default + traits are. The id is
        // preserved (resolution is by id), so the worker resolves the same
        // parameter the originating closure closed over.
        HeapObject::Parameter {
            id,
            default,
            traits,
        } => {
            let d = from_value_inner(*default, ctx)?;
            let t = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Parameter {
                id: *id,
                default: Box::new(d),
                traits: Box::new(t),
            })
        }

        // FFI type descriptors are pure data — safe to send
        HeapObject::FFIType(desc) => Ok(SendValue::FFIType(desc.clone())),

        // Bytes - immutable and safe to send, plus traits
        HeapObject::LBytes {
            data: b, traits, ..
        } => {
            let traits_sv = send_traits(*traits, HeapTag::LBytes, ctx)?;
            Ok(SendValue::Bytes(b.as_slice().to_vec(), Box::new(traits_sv)))
        }

        // @bytes - deep copy the bytes, plus traits
        HeapObject::LBytesMut {
            data: blob_ref,
            traits,
            ..
        } => {
            let borrowed = blob_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @bytes for sending".to_string())?;
            let traits_sv = send_traits(*traits, HeapTag::LBytesMut, ctx)?;
            Ok(SendValue::Blob(borrowed.clone(), Box::new(traits_sv)))
        }

        // Sets (immutable) - deep copy all elements, plus traits
        HeapObject::LSet {
            data: s, traits, ..
        } => {
            let copied: Result<Vec<SendValue>, String> =
                s.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = send_traits(*traits, HeapTag::LSet, ctx)?;
            Ok(SendValue::LSet(copied?, Box::new(traits_sv)))
        }

        // Sets (mutable) - deep copy all elements, plus traits
        HeapObject::LSetMut {
            data: s_ref,
            traits,
            ..
        } => {
            let borrowed = s_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow mutable set for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = send_traits(*traits, HeapTag::LSetMut, ctx)?;
            Ok(SendValue::LSetMut(copied?, Box::new(traits_sv)))
        }

        // A bare closure template is never a top-level user value (it is reached
        // only as a closure instance's `Region` template, serialized via the
        // Closure arm's `child_protos`), so it is never sent on its own.
        HeapObject::ClosureTemplate(_) => Err("Cannot send a bare closure template".to_string()),
    }
}
