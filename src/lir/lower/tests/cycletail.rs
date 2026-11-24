// audited: 2026-11-24
//! The slots a tail call carries for the closure-cycle arenas it strands.
//!
//! docs/impl/region/cycletail.md

use super::*;

/// A tail call that strands two merged arenas carries both slots, and each is a
/// slot its function merges.
///
/// The trap: an arena the call does not carry falls to the frame-exit hoist,
/// which replicates its release into the branch arms by value. The function
/// then names its merged slot in no `DecrefRegion`, and a debug build refuses
/// the compile.
#[test]
fn a_tail_call_carries_every_arena_it_strands() {
    use crate::hir::testkit::{TWO_CYCLES_NESTED, TWO_CYCLES_ONE_SCOPE};
    for source in [TWO_CYCLES_ONE_SCOPE, TWO_CYCLES_NESTED] {
        let module = compile_to_lir(&format!("{source} (f 3)"));
        let mut carried: Vec<(&LirOwned, Vec<StaticRegion>)> = Vec::new();
        for func in functions(&module) {
            for i in flat_instrs(func) {
                if let InstrRef::TailCall {
                    deferred_release_slots,
                    ..
                } = i
                {
                    if !deferred_release_slots.is_empty() {
                        carried.push((func, deferred_release_slots.iter().collect()));
                    }
                }
            }
        }
        assert_eq!(
            carried.len(),
            1,
            "one tail call strands merged arenas; got {:?}",
            carried.iter().map(|(_, s)| s).collect::<Vec<_>>(),
        );
        let (func, slots) = &carried[0];
        let merged = func.view().merged_slots();
        assert!(
            slots.len() == 2 && slots[0] != slots[1],
            "the tail call must carry both arenas' slots; got {slots:?}",
        );
        assert!(
            slots.iter().all(|s| merged.contains(s)),
            "each carried slot is a slot the function merges; slots={slots:?} \
             merged_slots={merged:?}",
        );
    }
}
