// audited: 2026-11-14
// docs/impl/region/operand.md
// docs/impl/jit.md
//! A compiled `DecrefPostponed`: a held postponed slot releases its value and stamps both slots, and an empty one changes nothing.

use super::*;
use crate::value::heap::{HeapObject, Pair};

/// fn(x): put `x` in local 0, the release's slot. Where `copied`, copy it into
/// local 1, the postponed slot, as an arm that postpones the release does. Then
/// run `DecrefPostponed { slot: 1, route: 0 }` and return local 0.
fn postponed_release_lir(copied: bool) -> LirOwned {
    let mut instrs = vec![
        InstrRef::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        InstrRef::StoreLocal {
            slot: 0,
            src: Reg(0),
        },
    ];
    if copied {
        instrs.push(InstrRef::LoadLocal {
            dst: Reg(1),
            slot: 0,
        });
        instrs.push(InstrRef::StoreLocal {
            slot: 1,
            src: Reg(1),
        });
    }
    instrs.push(InstrRef::DecrefPostponed { slot: 1, route: 0 });
    instrs.push(InstrRef::LoadLocal {
        dst: Reg(2),
        slot: 0,
    });
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .num_locals(2)
        .block(0, &instrs, Terminator::Return(Reg(2)))
        .build()
}

/// Compile `postponed_release_lir(copied)` and call it on a pair in a region
/// of its own: what it returns, and whether the region was freed.
fn run(copied: bool) -> (crate::value::Value, bool) {
    let lir = postponed_release_lir(copied);
    let code = verified().compile(&lir.view()).expect("Failed to compile");
    let mut vm = crate::vm::VM::new();
    let pair = HeapObject::Pair(Pair::new(
        crate::value::Value::NIL,
        crate::value::Value::NIL,
    ));
    let (x, region) = crate::value::arena::alloc_in_fresh_region(vm.heap(), pair);
    let generation = vm.heap().generation_raw(region.get());
    let args = [x];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    let freed = vm.heap().generation_raw(region.get()) != generation;
    if !freed {
        vm.heap().decref_region(region);
    }
    (value, freed)
}

/// The arm copied the release's slot into the postponed slot. The compiled
/// instruction releases the value and stamps the route, so the release behind
/// the call finds `nil`.
///
/// The counter-factual: a translation that left the route standing, so the
/// release behind the call ran a second time on the native fall-through.
#[test]
fn a_compiled_held_postponed_slot_releases_its_value_and_stamps_the_route() {
    let (value, freed) = run(true);
    assert!(value.is_nil(), "the route reads nil after the release");
    assert!(freed, "the release frees the region the pair held alone");
}

/// On a path through no arm that postpones the release, the postponed slot
/// reads `nil`. The compiled instruction releases nothing and leaves the route
/// to the release behind the call.
///
/// The counter-factual: a translation that stamped the route whatever the slot
/// held, so the release behind the call found `nil` and the region stranded.
#[test]
fn a_compiled_empty_postponed_slot_leaves_the_route_alone() {
    let (value, freed) = run(false);
    assert!(value.is_pair(), "the route still holds the pair");
    assert!(!freed, "nothing releases the region");
}
