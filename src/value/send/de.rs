use super::*;

/// Resolve a received traits value: if NIL, stamp the receiving thread's
/// default traitset for the given heap tag.
pub(super) fn recv_traits(
    heap: &crate::value::fiberheap::FiberHeap,
    traits_val: Value,
    tag: HeapTag,
) -> Value {
    if traits_val.is_nil() {
        crate::primitives::traitregistry::default_traits_for(heap, tag)
    } else {
        traits_val
    }
}

/// Reconstruct a symbol value on the receiving thread by re-interning its name
/// into the receiver's symbol table (`symbols`, threaded explicitly). A symbol
/// crosses the boundary by name because ids are per-table.
pub(super) fn recv_symbol(name: &str, symbols: &mut crate::symbol::SymbolTable) -> Value {
    Value::symbol(symbols.intern(name).0)
}

/// Reconstruct a standard-stream port value of the given kind. Only the three
/// stdio kinds are ever serialized (see the `"port"` arm in `from_value_inner`);
/// any other kind falls back to stdout defensively (unreachable in practice).
pub(super) fn stdio_port_value(
    kind: crate::port::PortKind,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Value {
    use crate::port::{Port, PortKind};
    let port = match kind {
        PortKind::Stdin => Port::stdin(),
        PortKind::Stderr => Port::stderr(),
        _ => Port::stdout(),
    };
    ctx.external("port", port)
}

/// Reconstruction state for a single intern table entry.
pub(super) enum ReconState {
    NotStarted,
    InProgress,
    Done(Value),
}

/// Per-call deserialization context for `SendBundle::into_value`.
///
/// Holds the receiving thread's `Alloc` allocation capability so every
/// reconstructed heap object is born explicitly in the call's region on the
/// call's heap. Region coherence is safety-critical here — a cross-thread message
/// tree must land entirely in one region (docs/impl/region/ctx.md).
pub(super) struct DeserContext<'a, 'h, 's> {
    /// Owned closure data. Entries are `take`n as they are reconstructed.
    closures: Vec<Option<SendableClosure>>,
    /// Reconstruction state per intern table index.
    pub(super) states: Vec<ReconState>,
    /// Deferred fixups: (LBox Value that holds a NIL placeholder, intern index).
    /// After all closures are built, each LBox's RefCell is overwritten with
    /// the actual closure value.
    pub(super) fixups: Vec<(Value, usize)>,
    /// The allocation capability every reconstructed heap object is born
    /// through — the receiving thread's per-call ctx.
    pub(super) ctx: &'a mut crate::primitives::ctx::Alloc<'h>,
    /// The RECEIVER's symbol table — a symbol value re-interns its name here on
    /// arrival (ids are per-table). Threaded explicitly
    /// (docs/impl/region/ctx.md § "Symbols through the ctx").
    pub(super) symbols: &'s mut crate::symbol::SymbolTable,
}

impl<'a, 'h, 's> DeserContext<'a, 'h, 's> {
    pub(super) fn new(
        closures: Vec<SendableClosure>,
        ctx: &'a mut crate::primitives::ctx::Alloc<'h>,
        symbols: &'s mut crate::symbol::SymbolTable,
    ) -> Self {
        let n = closures.len();
        DeserContext {
            closures: closures.into_iter().map(Some).collect(),
            states: (0..n).map(|_| ReconState::NotStarted).collect(),
            fixups: Vec::new(),
            ctx,
            symbols,
        }
    }

    /// Allocate a heap object into the reconstruction region.
    fn alloc(&self, obj: crate::value::heap::HeapObject) -> Value {
        self.ctx.alloc(obj)
    }

    /// Allocate a region-slice payload into the reconstruction region.
    fn alloc_slice<T: Copy + 'static>(
        &self,
        items: &[T],
    ) -> crate::value::region_slice::RegionSlice<T> {
        self.ctx.alloc_slice(items)
    }
}

/// Recursive worker for deserialization. Threads DeserContext through all recursive calls.
/// Reconstruct a closure **template** blueprint (a `SendableClosure` produced by
/// `sendable_from_template`) into an `Rc<ClosureTemplate>`. The inverse of
/// `sendable_from_template`: recurses on `child_protos` and ignores
/// `env`/`squelch_mask` (a blueprint is a pure template). Used to rebuild a
/// reconstructed template's `child_protos` so the worker's `MakeClosure`
/// resolves by index.
pub(in crate::value::send) fn template_from_sendable(
    sc: SendableClosure,
    ctx: &mut DeserContext<'_, '_, '_>,
) -> std::rc::Rc<crate::value::ClosureTemplate> {
    use std::rc::Rc;
    let constants: Vec<Value> = sc
        .constants
        .into_iter()
        .map(|cv| into_value_inner(cv, ctx))
        .collect();
    let doc = sc.doc.map(|d| std::rc::Rc::from(d.as_str()));
    let lir_value_pool: Vec<Value> = sc
        .lir_value_pool
        .into_iter()
        .map(|cv| into_value_inner(cv, ctx))
        .collect();
    let lir_function = sc.lir_function.map(|mut lir| {
        patch_lir_closure_refs(&mut lir, ctx);
        patch_lir_value_refs(&mut lir, &lir_value_pool);
        Rc::new(lir)
    });
    let child_protos: Vec<Rc<crate::value::ClosureTemplate>> = sc
        .child_protos
        .into_iter()
        .map(|p| template_from_sendable(p, ctx))
        .collect();
    Rc::new(crate::value::ClosureTemplate {
        num_locals: sc.num_locals,
        num_captures: sc.num_captures,
        num_params: sc.num_params,
        signal: sc.signal,
        capture_params_mask: sc.capture_params_mask,
        capture_locals_mask: sc.capture_locals_mask,
        symbol_names: Rc::new(sc.symbol_names),
        location_map: Rc::new(sc.location_map),
        lir_function,
        doc,
        vararg_kind: sc.vararg_kind,
        name: sc.name.map(|s| Rc::from(s.as_str())),
        child_protos: Rc::new(child_protos),
        merged_slots: Rc::new(sc.merged_slots.into_iter().collect()),
        frame_release_slots: Rc::new(sc.frame_release_slots),
        frame_release_regions: Rc::new(sc.frame_release_regions),
        ..crate::value::ClosureTemplate::new(Rc::new(sc.bytecode), sc.arity, Rc::new(constants))
    })
}

pub(super) fn into_value_inner(sv: SendValue, ctx: &mut DeserContext<'_, '_, '_>) -> Value {
    use crate::value::closure::{Closure, ClosureTemplate};
    use crate::value::heap::{HeapObject, Pair};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    match sv {
        SendValue::Immediate(v) => v,
        SendValue::Keyword(name) => Value::keyword(&name),
        SendValue::Symbol { name, .. } => recv_symbol(&name, ctx.symbols),
        SendValue::String(s) => ctx.ctx.string(s),
        SendValue::Syntax(ss) => ctx.ctx.syntax(send_to_syntax(*ss)),
        SendValue::Pair(first, rest, traits) => {
            let f = into_value_inner(*first, ctx);
            let r = into_value_inner(*rest, ctx);
            let traits_resolved = into_value_inner(*traits, ctx);
            let t = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::Pair);
            ctx.alloc(HeapObject::Pair(Pair {
                first: f,
                rest: r,
                traits: t,
            }))
        }
        SendValue::Array(items, traits) => {
            let values: Vec<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LArrayMut);
            ctx.alloc(HeapObject::LArrayMut {
                data: std::rc::Rc::new(RefCell::new(values)),
                traits: traits_val,
            })
        }
        SendValue::Struct(map, traits) => {
            // BTreeMap iterates in sorted order, so Vec is already sorted.
            let entries: Vec<_> = map
                .into_iter()
                .map(|(k, sv)| (k, into_value_inner(sv, ctx)))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LStruct);
            ctx.alloc(HeapObject::LStruct {
                data: entries,
                traits: traits_val,
            })
        }
        SendValue::StructMut(map, traits) => {
            let entries: BTreeMap<_, _> = map
                .into_iter()
                .map(|(k, sv)| (k, into_value_inner(sv, ctx)))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LStructMut);
            ctx.alloc(HeapObject::LStructMut {
                data: std::rc::Rc::new(RefCell::new(entries)),
                traits: traits_val,
            })
        }
        SendValue::Tuple(items, traits) => {
            let values: Vec<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LArray);
            let slice = ctx.alloc_slice::<Value>(&values);
            ctx.alloc(HeapObject::LArray {
                elements: slice,
                traits: traits_val,
            })
        }
        SendValue::Buffer(bytes, traits) => {
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LStringMut);
            ctx.alloc(HeapObject::LStringMut {
                data: std::rc::Rc::new(RefCell::new(bytes)),
                traits: traits_val,
            })
        }
        SendValue::Bytes(bytes, traits) => {
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LBytes);
            let slice = ctx.alloc_slice::<u8>(&bytes);
            ctx.alloc(HeapObject::LBytes {
                data: slice,
                traits: traits_val,
            })
        }
        SendValue::Blob(bytes, traits) => {
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LBytesMut);
            ctx.alloc(HeapObject::LBytesMut {
                data: std::rc::Rc::new(RefCell::new(bytes)),
                traits: traits_val,
            })
        }

        SendValue::LBox(contents, traits) => {
            let fixup_idx = match *contents {
                SendValue::Ref(idx) => {
                    if matches!(ctx.states[idx], ReconState::InProgress) {
                        Some(idx)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let inner_val = into_value_inner(*contents, ctx);
            let traits_val = into_value_inner(*traits, ctx);
            let lbox_val = ctx.alloc(HeapObject::LBox {
                cell: std::rc::Rc::new(RefCell::new(inner_val)),
                traits: traits_val,
            });
            if let Some(idx) = fixup_idx {
                ctx.fixups.push((lbox_val, idx));
            }
            lbox_val
        }

        SendValue::CaptureCell(contents, traits) => {
            let fixup_idx = match *contents {
                SendValue::Ref(idx) => {
                    if matches!(ctx.states[idx], ReconState::InProgress) {
                        Some(idx)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let inner_val = into_value_inner(*contents, ctx);
            let traits_val = into_value_inner(*traits, ctx);
            let cell_val = ctx.alloc(HeapObject::CaptureCell {
                cell: std::rc::Rc::new(RefCell::new(inner_val)),
                traits: traits_val,
            });
            if let Some(idx) = fixup_idx {
                ctx.fixups.push((cell_val, idx));
            }
            cell_val
        }

        SendValue::Float(f) => ctx.alloc(HeapObject::Float(f)),
        SendValue::FFIType(desc) => ctx.alloc(HeapObject::FFIType(desc)),
        SendValue::LSet(items, traits) => {
            let set: BTreeSet<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LSet);
            // BTreeSet iterates in sorted order; collect into Vec and copy into arena.
            let sorted: Vec<Value> = set.into_iter().collect();
            let slice = ctx.alloc_slice::<Value>(&sorted);
            ctx.alloc(HeapObject::LSet {
                data: slice,
                traits: traits_val,
            })
        }
        SendValue::LSetMut(items, traits) => {
            let set: BTreeSet<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_resolved = into_value_inner(*traits, ctx);
            let traits_val = recv_traits(ctx.ctx.heap_mut(), traits_resolved, HeapTag::LSetMut);
            ctx.alloc(HeapObject::LSetMut {
                data: std::rc::Rc::new(RefCell::new(set)),
                traits: traits_val,
            })
        }
        SendValue::ChanSender(tx, wake) => crate::primitives::chan::sender_value(tx, wake, ctx.ctx),
        SendValue::ChanReceiver(rx, wake) => {
            crate::primitives::chan::receiver_value(rx, wake, ctx.ctx)
        }
        SendValue::StdioPort(kind) => stdio_port_value(kind, ctx.ctx),
        SendValue::Parameter {
            id,
            default,
            traits,
        } => {
            let default = into_value_inner(*default, ctx);
            let traits = into_value_inner(*traits, ctx);
            ctx.alloc(HeapObject::Parameter {
                id,
                default,
                traits,
            })
        }

        // Closure variant: only appears stored directly in SendBundle::closures.
        // At the top-level call it means the bundle was constructed incorrectly.
        // In practice root is always a Ref when the value is a closure.
        SendValue::Closure(_box_val) => panic!("bug: bare Closure in SendValue tree; use Ref"),

        SendValue::Ref(idx) => {
            if let ReconState::Done(v) = ctx.states[idx] {
                return v;
            }
            if matches!(ctx.states[idx], ReconState::InProgress) {
                return Value::NIL; // placeholder; fixup will correct it
            }
            // NotStarted — fall through to reconstruct

            ctx.states[idx] = ReconState::InProgress;
            let sc = ctx.closures[idx]
                .take()
                .expect("bug: closure already taken from DeserContext");

            // Reconstruct constants (no closures expected in constants,
            // but thread the context for completeness).
            let constants: Vec<Value> = sc
                .constants
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();

            // Reconstruct env (may encounter InProgress Refs → NIL placeholders).
            let env: Vec<Value> = sc
                .env
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();

            let doc = sc.doc.map(|d| std::rc::Rc::from(d.as_str()));

            // Rebuild the compound-value pool lifted out of the LIR on send.
            let lir_value_pool: Vec<Value> = sc
                .lir_value_pool
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();

            // Patch the LIR placeholders back to ValueConst: ClosureRef entries
            // (forcing referenced closures to reconstruct) and ValueRef entries
            // (from the pool above). Both invert convert_lir_for_send.
            let lir_function = sc.lir_function.map(|mut lir| {
                patch_lir_closure_refs(&mut lir, ctx);
                patch_lir_value_refs(&mut lir, &lir_value_pool);
                Rc::new(lir)
            });

            // Reconstruct the nested-lambda blueprints so this template's
            // `MakeClosure`s resolve by index in the worker.
            let child_protos: Vec<Rc<ClosureTemplate>> = sc
                .child_protos
                .into_iter()
                .map(|p| template_from_sendable(p, ctx))
                .collect();

            let template = Rc::new(ClosureTemplate {
                num_locals: sc.num_locals,
                num_captures: sc.num_captures,
                num_params: sc.num_params,
                signal: sc.signal,
                capture_params_mask: sc.capture_params_mask,
                capture_locals_mask: sc.capture_locals_mask,
                symbol_names: Rc::new(sc.symbol_names),
                location_map: Rc::new(sc.location_map),
                lir_function,
                doc,
                vararg_kind: sc.vararg_kind,
                name: sc.name.map(|s| Rc::from(s.as_str())),
                child_protos: Rc::new(child_protos),
                merged_slots: Rc::new(sc.merged_slots.into_iter().collect()),
                frame_release_slots: Rc::new(sc.frame_release_slots),
                frame_release_regions: Rc::new(sc.frame_release_regions),
                ..ClosureTemplate::new(Rc::new(sc.bytecode), sc.arity, Rc::new(constants))
            });

            let env_slice = ctx.alloc_slice::<Value>(&env);
            let val = ctx.ctx.closure(Closure {
                template: crate::value::TemplateRef::new(template),
                env: env_slice,
                squelch_mask: sc.squelch_mask,
            });
            ctx.states[idx] = ReconState::Done(val);
            val
        }
    }
}

/// Patch `ClosureRef(idx)` entries in a LIR function back to `ValueConst`.
/// Forces reconstruction of any referenced closures that haven't been built yet.
fn patch_lir_closure_refs(lir: &mut crate::lir::LirFunction, ctx: &mut DeserContext<'_, '_, '_>) {
    use crate::lir::LirConst;
    use crate::lir::LirInstr;

    for block in &mut lir.blocks {
        for si in &mut block.instructions {
            if let LirInstr::Const {
                dst,
                value: LirConst::ClosureRef(ref_idx),
            } = &si.instr
            {
                let ref_idx = *ref_idx;
                let dst = *dst;
                // Ensure the referenced closure is reconstructed.
                let closure_val = match ctx.states[ref_idx] {
                    ReconState::Done(v) => v,
                    _ => {
                        // Force reconstruction via a Ref lookup.
                        into_value_inner(SendValue::Ref(ref_idx), ctx)
                    }
                };
                si.instr = LirInstr::ValueConst {
                    dst,
                    value: closure_val,
                };
            }
        }
    }
}

/// Patch `ValueRef(idx)` entries in a LIR function back to `ValueConst`, using
/// the reconstructed compound-value `pool`. Inverts `convert_lir_for_send`'s
/// pass 1; mirrors `patch_lir_closure_refs`.
fn patch_lir_value_refs(lir: &mut crate::lir::LirFunction, pool: &[Value]) {
    use crate::lir::LirConst;
    use crate::lir::LirInstr;

    for block in &mut lir.blocks {
        for si in &mut block.instructions {
            if let LirInstr::Const {
                dst,
                value: LirConst::ValueRef(ref_idx),
            } = &si.instr
            {
                let value = pool[*ref_idx];
                let dst = *dst;
                si.instr = LirInstr::ValueConst { dst, value };
            }
        }
    }
}
