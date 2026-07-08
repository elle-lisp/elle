//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::regionstore::RegionStore;
use crate::value::heap::{HeapObject, HeapTag, Pair};

/// Which `Value` channels a variant exposes to the scan. Declared per
/// arm in [`obj_with_value_in_every_channel`] — the single source of
/// truth the pin asserts against.
struct Channels {
    /// Variant content (elements, cell, env, constants, default, …)
    /// carries the region-2 value; the scan must report region 2.
    content: bool,
    /// The `traits` field carries the region-4 value; the scan must
    /// report region 4. (Only `ClosureTemplate` and the value-free
    /// variants lack a traits field.)
    traits: bool,
}

/// Construct one instance of `tag` with a cross-region value in EVERY
/// `Value` channel the variant has: `v2` in each content channel, `vt`
/// in the `traits` channel. The two live in DIFFERENT regions so each
/// channel is individually load-bearing — a dropped content arm cannot
/// hide behind the traits edge or vice versa. Returns `None` for
/// variants with no channel at all (provably value-free: the scan must
/// find nothing in them).
///
/// Exhaustive by construction: a new `HeapObject` variant does not
/// compile until it gets an arm here, i.e. an explicit scan decision
/// (docs/impl/region/diagnostics.md § Validation — the exhaustive-scan pin).
fn obj_with_value_in_every_channel(
    tag: HeapTag,
    v2: Value,
    vt: Value,
    store: &mut RegionStore,
    own: RuntimeRegion,
) -> Option<(HeapObject, Channels)> {
    use std::cell::RefCell;
    use std::rc::Rc;
    let both = Channels {
        content: true,
        traits: true,
    };
    let traits_only = Channels {
        content: false,
        traits: true,
    };
    let vals_in_own = |store: &mut RegionStore| store.alloc_region_slice(own, &[v2]);
    let table_key = || crate::value::TableKey::from_value(&Value::int(1)).unwrap();
    let empty_template = || {
        Rc::new(crate::value::closure::ClosureTemplate::new(
            Rc::new(vec![]),
            crate::value::Arity::Exact(0),
            Rc::new(vec![]),
        ))
    };
    Some(match tag {
        HeapTag::LString => (
            HeapObject::LString {
                s: RegionSlice::empty(),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::Pair => {
            let mut pair = Pair::new(v2, v2);
            pair.traits = vt;
            (HeapObject::Pair(pair), both)
        }
        HeapTag::LArrayMut => (
            HeapObject::LArrayMut {
                data: Rc::new(RefCell::new(vec![v2])),
                traits: vt,
            },
            both,
        ),
        HeapTag::LStructMut => (
            HeapObject::LStructMut {
                data: Rc::new(RefCell::new(std::collections::BTreeMap::from([(
                    table_key(),
                    v2,
                )]))),
                traits: vt,
            },
            both,
        ),
        HeapTag::LStruct => (
            HeapObject::LStruct {
                data: vec![(table_key(), v2)],
                traits: vt,
            },
            both,
        ),
        HeapTag::Closure => (
            // Env contents are the channel; the env backing itself lives
            // in `own` (the usual co-region layout), so only `v2`'s
            // region is a cross edge. A Shared (Rc) template has no
            // region Value — the Region-template edge is the
            // ClosureTemplate arm's channel below.
            HeapObject::Closure {
                closure: crate::value::closure::Closure::new(
                    empty_template(),
                    vals_in_own(store),
                    crate::value::SignalBits::EMPTY,
                ),
                traits: vt,
            },
            both,
        ),
        HeapTag::Syntax => (
            // Syntax holds only compile-time data (String / Vec<Syntax>
            // / Rc<Syntax>) — no `Value` content channel; traits only.
            HeapObject::Syntax {
                syntax: Box::new(crate::syntax::Syntax {
                    kind: crate::syntax::SyntaxKind::Nil,
                    span: crate::syntax::Span::new(0, 0, 0, 0),
                    scopes: Vec::new(),
                    scope_exempt: false,
                }),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::LArray => (
            HeapObject::LArray {
                elements: vals_in_own(store),
                traits: vt,
            },
            both,
        ),
        HeapTag::LBox => (
            HeapObject::LBox {
                cell: Rc::new(RefCell::new(v2)),
                traits: vt,
            },
            both,
        ),
        // Value-free by construction: no `Value` field, no traits.
        HeapTag::Float => return None,
        HeapTag::LibHandle => return None,
        HeapTag::ThreadHandle => (
            // Result/channel traffic crosses threads as deep-copied
            // SendBundles, never as region Values; traits only.
            HeapObject::ThreadHandle {
                handle: crate::value::heap::ThreadHandle {
                    result: std::sync::Arc::new(std::sync::Mutex::new(None)),
                    done_rx: crossbeam_channel::unbounded().1,
                    done_wake: crate::primitives::chan::WakeList::new(),
                },
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::Fiber => {
            // A parked fiber's channels: its closure's env contents (and
            // backing), its template, and its park-retained terminal
            // signal value. Empty env + Shared template here; the signal
            // value is the content channel under test (SIG_OK is
            // terminal).
            let closure = crate::value::closure::Closure::new(
                empty_template(),
                RegionSlice::empty(),
                crate::value::SignalBits::EMPTY,
            );
            let mut fiber =
                crate::value::fiber::Fiber::new(Rc::new(closure), crate::value::SignalBits::EMPTY);
            fiber.signal = Some((crate::signals::SIG_OK, v2));
            (
                HeapObject::Fiber {
                    handle: crate::value::fiber::FiberHandle::new(fiber),
                    traits: vt,
                },
                both,
            )
        }
        HeapTag::FFISignature => return None,
        HeapTag::FFIType => return None,
        HeapTag::ManagedPointer => (
            HeapObject::ManagedPointer {
                addr: std::cell::Cell::new(None),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::LStringMut => (
            HeapObject::LStringMut {
                data: Rc::new(RefCell::new(Vec::new())),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::LBytes => (
            HeapObject::LBytes {
                data: RegionSlice::empty(),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::LBytesMut => (
            HeapObject::LBytesMut {
                data: Rc::new(RefCell::new(Vec::new())),
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::External => (
            // The Rc<dyn Any> payload is opaque BY CONSTRUCTION — a
            // plugin storing region Values inside it hides them from the
            // scan (docs/impl/region/diagnostics.md § Validation names this boundary).
            // Traits is the only visible channel.
            HeapObject::External {
                obj: crate::value::heap::ExternalObject {
                    type_name: "scan-pin",
                    data: Rc::new(0u8),
                },
                traits: vt,
            },
            traits_only,
        ),
        HeapTag::Parameter => (
            HeapObject::Parameter {
                id: 0,
                default: v2,
                traits: vt,
            },
            both,
        ),
        HeapTag::LSet => (
            HeapObject::LSet {
                data: vals_in_own(store),
                traits: vt,
            },
            both,
        ),
        HeapTag::LSetMut => (
            HeapObject::LSetMut {
                data: Rc::new(RefCell::new(std::collections::BTreeSet::from([v2]))),
                traits: vt,
            },
            both,
        ),
        HeapTag::CaptureCell => (
            HeapObject::CaptureCell {
                cell: Rc::new(RefCell::new(v2)),
                traits: vt,
            },
            both,
        ),
        HeapTag::ClosureTemplate => (
            // The constant pool is the template's only channel (no
            // traits field; child_protos are plain Rc data).
            HeapObject::ClosureTemplate(crate::value::closure::ClosureTemplate::new(
                Rc::new(vec![]),
                crate::value::Arity::Exact(0),
                Rc::new(vec![v2]),
            )),
            Channels {
                content: true,
                traits: false,
            },
        ),
    })
}

/// Every `HeapTag`, for iteration. Completeness is NOT load-bearing:
/// the exhaustive `match` above is what forces a decision for a new
/// variant (at compile time); this list only drives the runtime loop.
const ALL_TAGS: &[HeapTag] = &[
    HeapTag::LString,
    HeapTag::Pair,
    HeapTag::LArrayMut,
    HeapTag::LStructMut,
    HeapTag::LStruct,
    HeapTag::Closure,
    HeapTag::Syntax,
    HeapTag::LArray,
    HeapTag::LBox,
    HeapTag::Float,
    HeapTag::LibHandle,
    HeapTag::ThreadHandle,
    HeapTag::Fiber,
    HeapTag::FFISignature,
    HeapTag::FFIType,
    HeapTag::ManagedPointer,
    HeapTag::LStringMut,
    HeapTag::LBytes,
    HeapTag::LBytesMut,
    HeapTag::External,
    HeapTag::Parameter,
    HeapTag::LSet,
    HeapTag::LSetMut,
    HeapTag::CaptureCell,
    HeapTag::ClosureTemplate,
];

/// The exhaustive-scan pin (docs/impl/region/diagnostics.md § Validation): every
/// variant that CAN hold a cross-region `Value` must surface it through
/// `find_object_cross_refs` — each channel independently — and every
/// variant that can't must surface nothing. A missing scan arm fails
/// here, not in review; the missing edge it would have caused (no
/// alloc-time incref, no free-time cascade decref) is a premature free.
#[test]
fn exhaustive_scan_finds_cross_region_refs_in_every_variant() {
    let mut store = RegionStore::default();
    let r2 = store.new_runtime_region();
    let v2 = store.alloc_obj(r2, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let rt = store.new_runtime_region();
    let vt = store.alloc_obj(rt, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let own = store.new_runtime_region();
    // `own` must exist so its self-edges are real (and filtered).
    store.alloc_obj(own, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));

    let scan = |obj: &HeapObject, store: &RegionStore| {
        let mut refs = Vec::new();
        RegionPool::find_object_cross_refs(
            obj,
            own.get(),
            store.page_size(),
            &|_, _| true,
            &mut refs,
        );
        refs
    };

    for &tag in ALL_TAGS {
        match obj_with_value_in_every_channel(tag, v2, vt, &mut store, own) {
            Some((obj, channels)) => {
                let refs = scan(&obj, &store);
                if channels.content {
                    assert!(
                        refs.contains(&r2.get()),
                        "scan missed the content cross-region ref in {tag:?} \
                             (found {refs:?}): a value stored there gets no alloc-time \
                             incref and no free-time cascade decref — premature free"
                    );
                } else {
                    assert!(
                        !refs.contains(&r2.get()),
                        "scan found a content ref in {tag:?}, which declares \
                             no content channel — update the pin's Channels"
                    );
                }
                if channels.traits {
                    assert!(
                        refs.contains(&rt.get()),
                        "scan missed the traits cross-region ref in {tag:?} \
                             (found {refs:?})"
                    );
                }
                assert!(
                    !refs.contains(&own.get()),
                    "scan reported a self-edge for {tag:?}"
                );
            }
            None => {
                // Value-free variants: the scan must stay silent.
                let obj = match tag {
                    HeapTag::Float => HeapObject::Float(1.0),
                    HeapTag::LibHandle => HeapObject::LibHandle(0),
                    HeapTag::FFISignature => HeapObject::FFISignature(
                        crate::ffi::types::Signature {
                            convention: crate::ffi::types::CallingConvention::Default,
                            ret: crate::ffi::types::TypeDesc::Void,
                            args: vec![],
                            fixed_args: None,
                        },
                        crate::value::heap::CifCache::default(),
                    ),
                    HeapTag::FFIType => HeapObject::FFIType(crate::ffi::types::TypeDesc::Void),
                    other => unreachable!(
                        "{other:?} returned None from obj_with_value_in_every_channel \
                             but has no value-free construction here"
                    ),
                };
                let refs = scan(&obj, &store);
                assert!(
                    refs.is_empty(),
                    "scan found refs {refs:?} in supposedly value-free {tag:?}"
                );
            }
        }
    }
}
