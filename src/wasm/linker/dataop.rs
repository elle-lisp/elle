use super::*;

/// Read (tag, payload) pairs from linear memory at `regs_ptr`.
pub(super) fn read_reg_pairs(
    caller: &mut Caller<'_, ElleHost>,
    regs_ptr: i32,
    num_regs: i32,
) -> Vec<(i64, i64)> {
    if num_regs <= 0 {
        return Vec::new();
    }
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("read_reg_pairs: no memory");
    let data = memory.data(&*caller);
    let mut pairs = Vec::with_capacity(num_regs as usize);
    for i in 0..num_regs as usize {
        let offset = regs_ptr as usize + i * 16;
        let tag = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        let payload = i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap());
        pairs.push((tag, payload));
    }
    pairs
}
/// Dispatch a data operation by opcode. `vm` is the host's driving VM, so the
/// intrinsic arms (`IntrLength`/`Get`/`Put`/…) run their `PrimFn` bodies through a
/// VM-bearing `NativeCtx` (docs/impl/region/ctx.md).
///
/// `vm` is a raw pointer rather than a reference because it arrives from the wasm
/// runtime's call trampoline; the caller holds the live VM for the call's
/// duration, so the deref below is sound. The unsafe is localized to that deref
/// (as with the VM methods that read `self.heap_ptr`) rather than promoted to an
/// `unsafe fn`, which would only push the same contract onto the two in-crate
/// callers.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn dispatch_data_op(
    op: i32,
    args: &[Value],
    vm: *mut crate::vm::VM,
) -> (crate::value::fiber::SignalBits, Value) {
    use super::super::emit::DataOp;
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    use crate::value::heap::TableKey;
    use crate::value::sorted_struct_get;

    let heap = unsafe { &mut *(*vm).heap_ptr };
    let region = heap.new_runtime_region();
    let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm);

    let err = |kind: &str, msg: &str| (SIG_ERROR, ctx.error(kind, msg));

    match op {
        x if x == DataOp::Pair as i32 => (SIG_OK, ctx.pair(args[0], args[1])),
        x if x == DataOp::First as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::Rest as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::MatchFail as i32 => (SIG_ERROR, ctx.match_fail(args[0])),
        x if x == DataOp::FirstDestructure as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.first),
            None => err("type-error", "first: not a pair"),
        },
        x if x == DataOp::RestDestructure as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.rest),
            None => err("type-error", "rest: not a pair"),
        },
        x if x == DataOp::FirstOrNil as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::RestOrNil as i32 => match args[0].as_pair() {
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::EMPTY_LIST),
        },
        x if x == DataOp::MakeArray as i32 => (SIG_OK, ctx.array_mut(args.to_vec())),
        x if x == DataOp::MakeCapture as i32 => (SIG_OK, ctx.capture_cell(args[0])),
        x if x == DataOp::LoadCapture as i32 => match args[0].as_capture_cell() {
            Some(cell) => (SIG_OK, *cell.borrow()),
            None => (SIG_OK, args[0]),
        },
        x if x == DataOp::StoreCapture as i32 => {
            if args[0].is_capture_cell() {
                // The capture-store funnel (Rule 5): region RC ops are
                // no-ops in the WASM tier (no region instructions are
                // emitted), but the one store routine keeps the tiers
                // semantically identical.
                crate::value::arena::capture_store_with_rebind(ctx.heap_mut(), args[0], args[1]);
            }
            (SIG_OK, Value::NIL)
        }
        11 => (SIG_OK, Value::NIL), // MakeString (unused)
        x if x == DataOp::ArrayRefDestructure as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                if index < b.len() {
                    (SIG_OK, b[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else if let Some(arr) = args[0].as_array() {
                if index < arr.len() {
                    (SIG_OK, arr[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else {
                err("type-error", "array ref: not an array")
            }
        }
        x if x == DataOp::ArraySliceFrom as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, ctx.array_mut(b[index.min(b.len())..].to_vec()))
            } else if let Some(arr) = args[0].as_array() {
                (SIG_OK, ctx.array_mut(arr[index.min(arr.len())..].to_vec()))
            } else {
                (SIG_OK, ctx.array_mut(vec![]))
            }
        }
        x if x == DataOp::StructGetOrNil as i32 => {
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (
                    SIG_OK,
                    sorted_struct_get(s, &key).copied().unwrap_or(Value::NIL),
                )
            } else if let Some(s) = args[0].as_struct_mut() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (SIG_OK, s.borrow().get(&key).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        x if x == DataOp::StructGetDestructure as i32 => {
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                match sorted_struct_get(s, &key) {
                    Some(v) => (SIG_OK, *v),
                    None => err("key-error", "struct get: key not found"),
                }
            } else {
                err("type-error", "struct get: not a struct")
            }
        }
        x if x == DataOp::ArrayExtend as i32 => {
            if let Some(arr) = args[0].as_array_mut() {
                let source_elems: Vec<Value> = if let Some(src) = args[1].as_array_mut() {
                    src.borrow().to_vec()
                } else if let Some(src) = args[1].as_array() {
                    src.to_vec()
                } else if args[1].as_pair().is_some() || args[1].is_empty_list() {
                    match args[1].list_to_vec() {
                        Ok(v) => v,
                        Err(_) => {
                            return err("type-error", "splice: not a proper list");
                        }
                    }
                } else {
                    return err(
                        "type-error",
                        &format!(
                            "splice: expected array or list, got {}",
                            args[1].type_name()
                        ),
                    );
                };
                let mut vec = arr.borrow().to_vec();
                vec.extend(source_elems);
                (SIG_OK, ctx.array_mut(vec))
            } else {
                (SIG_OK, args[0])
            }
        }
        x if x == DataOp::ArrayPush as i32 => {
            if args[0].is_array_mut() {
                // Tracked funnel — see StoreCapture above.
                crate::value::arena::push_with_incref(ctx.heap_mut(), args[0], args[1]);
            }
            (SIG_OK, args[0])
        }
        x if x == DataOp::ArrayLen as i32 => {
            let len = if let Some(arr) = args[0].as_array_mut() {
                arr.borrow().len()
            } else if let Some(arr) = args[0].as_array() {
                arr.len()
            } else {
                0
            };
            (SIG_OK, Value::int(len as i64))
        }
        x if x == DataOp::ArrayRefOrNil as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, b.get(index).copied().unwrap_or(Value::NIL))
            } else if let Some(arr) = args[0].as_array() {
                (SIG_OK, arr.get(index).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        x if x == DataOp::StructRest as i32 => {
            let exclude_keys: Vec<TableKey> =
                args[1..].iter().filter_map(TableKey::from_value).collect();
            if let Some(s) = args[0].as_struct() {
                let filtered: Vec<(TableKey, Value)> = s
                    .iter()
                    .filter(|(k, _)| !exclude_keys.contains(k))
                    .map(|(k, v)| (*k, *v))
                    .collect();
                (SIG_OK, ctx.struct_from_sorted(filtered))
            } else if let Some(s) = args[0].as_struct_mut() {
                let b = s.borrow();
                let mut filtered: Vec<(TableKey, Value)> = b
                    .iter()
                    .filter(|(k, _)| !exclude_keys.contains(k))
                    .map(|(k, v)| (*k, *v))
                    .collect();
                filtered.sort_by_key(|(a, _)| *a);
                (SIG_OK, ctx.struct_from_sorted(filtered))
            } else {
                (SIG_OK, ctx.struct_from_sorted(vec![]))
            }
        }
        x if x == DataOp::IntToFloat as i32 => {
            if let Some(n) = args[0].as_int() {
                (SIG_OK, Value::float(n as f64))
            } else if args[0].as_float().is_some() {
                (SIG_OK, args[0])
            } else {
                err(
                    "type-error",
                    &format!("float: expected number, got {}", args[0].type_name()),
                )
            }
        }
        x if x == DataOp::FloatToInt as i32 => {
            if let Some(f) = args[0].as_float() {
                (SIG_OK, Value::int(f as i64))
            } else if args[0].as_int().is_some() {
                (SIG_OK, args[0])
            } else {
                err(
                    "type-error",
                    &format!("integer: expected number, got {}", args[0].type_name()),
                )
            }
        }
        x if x == DataOp::IntrTypeOf as i32 => (SIG_OK, Value::keyword(args[0].type_name())),
        // Intrinsic data ops — contract: caller provides correct types.
        // Wrong types → panic (not a signal).
        x if x == DataOp::IntrLength as i32 => {
            let (b, r) = crate::primitives::list::prim_length(&mut ctx, &args[..1]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%length: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrGetOp as i32 => {
            let (b, r) = crate::primitives::access::prim_get(&mut ctx, &args[..2]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%get: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrPutOp as i32 => {
            let (b, r) = crate::primitives::access::prim_put(&mut ctx, &args[..3]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%put: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrDelOp as i32 => {
            let (b, r) = crate::primitives::lstruct::prim_del(&mut ctx, &args[..2]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%del: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrHasOp as i32 => {
            let (b, r) = crate::primitives::lstruct::prim_has_key(&mut ctx, &args[..2]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%has?: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrPushOp as i32 => {
            let (b, r) = crate::primitives::array::prim_push(&mut ctx, &args[..2]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%array-push: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrStringPushOp as i32 => {
            panic!("not yet implemented: IntrStringPush in WASM linker");
        }
        x if x == DataOp::IntrBytesPushOp as i32 => {
            panic!("not yet implemented: IntrBytesPush in WASM linker");
        }
        x if x == DataOp::IntrPopOp as i32 => {
            let (b, r) = crate::primitives::array::prim_pop(&mut ctx, &args[..1]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%pop: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrFreezeOp as i32 => {
            let (b, r) = crate::primitives::structs::prim_freeze(&mut ctx, &args[..1]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%freeze: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrThawOp as i32 => {
            let (b, r) = crate::primitives::structs::prim_thaw(&mut ctx, &args[..1]);
            assert!(
                !b.intersects(SIG_ERROR),
                "%thaw: intrinsic contract violated"
            );
            (SIG_OK, r)
        }
        x if x == DataOp::IntrIdenticalOp as i32 => (
            SIG_OK,
            Value::bool(args[0].tag == args[1].tag && args[0].payload == args[1].payload),
        ),
        _ => err("internal-error", &format!("rt_data_op: unknown op {op}")),
    }
}
/// Read args from linear memory as `Vec<Value>`.
pub(super) fn read_args_from_memory(
    caller: &mut Caller<'_, ElleHost>,
    args_ptr: i32,
    nargs: i32,
) -> Vec<Value> {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory());
    let memory = match memory {
        Some(m) => m,
        None => return Vec::new(),
    };
    // A Call's arg count is a u16 in the front end (a >128-field struct literal
    // is a >256-arg `struct` call), so nargs ranges over `0..=u16::MAX`. The args
    // region cannot collide with a live closure env because the env stack begins
    // above the module's widest args region (emit::env_stack_base); this bound is
    // just a sanity guard against a corrupt/garbage count.
    assert!(
        (0..=u16::MAX as i32).contains(&nargs),
        "read_args_from_memory: invalid nargs={} args_ptr={}",
        nargs,
        args_ptr
    );
    let data = memory.data(&*caller);
    super::super::handle::read_args_from_slice(
        data,
        &caller.data().handles,
        args_ptr as usize,
        nargs as usize,
    )
}
