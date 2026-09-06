// audited: 2026-09-05
//! Which payload a header reads, and how long the region behind it lives.
//! docs/impl/region/template.md
//!
//! The helpers here serve every themed submodule, so each one's `use super::*;`
//! resolves the same names.

use std::rc::Rc;

pub(crate) use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;

mod payload;

/// A fresh region to build a header in.
///
/// The trap: a raw `RuntimeRegion::new(2)` is NOT safe here. An id is live only
/// once something is allocated into it, and the payload cache mints its region
/// from the same counter — so a hand-picked id the test has not allocated into
/// can be handed straight back as the payload's region, silently making the
/// header co-region with its payload and every cross-region assertion below
/// vacuous.
fn region(heap: &mut FiberHeap) -> RuntimeRegion {
    heap.new_runtime_region()
}

/// Materialize a header for `p` into a fresh region of `heap`.
fn header_in(heap: &mut FiberHeap, p: &Rc<TemplateProto>) -> Value {
    let region = region(heap);
    materialize(heap, p, region)
}

/// A blueprint whose bytecode is long enough that a per-creation copy would be
/// visible as a distinct backing address.
fn proto(bytecode: Vec<u8>) -> Rc<TemplateProto> {
    Rc::new(TemplateProto::new(
        bytecode,
        Arity::Exact(0),
        vec![Value::int(7), Value::int(8)],
    ))
}

/// Read the `ClosureTemplate` header out of a materialized template `Value`.
fn header(v: Value) -> &'static ClosureTemplate {
    let obj: &'static HeapObject = unsafe { crate::value::arena::deref(v) };
    match obj {
        HeapObject::ClosureTemplate(t) => t,
        other => panic!("expected a ClosureTemplate, got {}", other.type_name()),
    }
}

/// One blueprint has one payload, however many headers are built from it. This
/// is the whole reason the payload is split out of the header: `MakeClosure`
/// runs once per closure *creation*, so a closure built in a loop would copy
/// its function's whole bytecode per iteration if the payload rode along.
///
/// The counter-factual is a payload copied per materialization: the two
/// headers would then report different backing addresses for identical bytes,
/// and the assertion below would compare unequal pointers.
#[test]
fn two_headers_from_one_blueprint_share_one_payload() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let first = header(header_in(&mut heap, &p));
    let second = header(header_in(&mut heap, &p));

    assert_eq!(
        first.bytecode(),
        second.bytecode(),
        "two headers from one blueprint must read the same bytecode"
    );
    assert_eq!(
        first.bytecode().as_ptr(),
        second.bytecode().as_ptr(),
        "the payload is materialized once per blueprint and shared; a second \
         backing address means it was copied per header"
    );
    assert_eq!(
        first.constants().as_ptr(),
        second.constants().as_ptr(),
        "the constant pool is payload, shared with the bytecode"
    );
}

/// Sharing is per blueprint, not global: two blueprints never collapse onto one
/// payload even when their bytes are identical. Without this the cache would be
/// a content-addressed store, and a header could not name its own code.
#[test]
fn each_blueprint_gets_its_own_payload() {
    let mut heap = FiberHeap::new();
    let a = proto(vec![9, 9, 9, 9]);
    let b = proto(vec![9, 9, 9, 9]);

    let ha = header(header_in(&mut heap, &a));
    let hb = header(header_in(&mut heap, &b));

    assert_eq!(
        ha.bytecode(),
        hb.bytecode(),
        "identical bytes, by construction"
    );
    assert_ne!(
        ha.bytecode().as_ptr(),
        hb.bytecode().as_ptr(),
        "two blueprints must not share one payload"
    );
}

/// The payload lives in a region of the heap's own, not in the header's region,
/// so the header's reference to it is an ordinary counted cross-region edge
/// (Rule 5). Allocating the header increfs the payload region; freeing the
/// header's region decrefs it again.
///
/// The counter-factual is a payload backing that the alloc scan does not see:
/// the payload region's RC would stay at the cache's single reference, and
/// freeing it while a header still named it would be a use-after-free.
#[test]
fn a_header_increfs_the_region_holding_its_payload() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![1, 2, 3, 4]);

    let tv = header_in(&mut heap, &p);
    let payload_region =
        RuntimeRegion::new(heap.region_of_ptr(header(tv).bytecode().as_ptr() as *const ()))
            .expect("the payload lives in a real region");

    let held = heap.region_rc(payload_region);
    assert!(
        held >= 2,
        "the cache holds one reference and the header takes another; rc was {held}"
    );

    // A second header in a third region takes its own reference.
    let _ = header_in(&mut heap, &p);
    assert_eq!(
        heap.region_rc(payload_region),
        held + 1,
        "every header naming the payload takes its own counted reference"
    );
}

/// A header can outlive the region a sibling header was born in. Freeing one
/// header's region must not take the shared payload with it — the surviving
/// header still reads its bytecode.
///
/// The trap this guards: the payload backing is a `RegionSlice` copied by value
/// into each header, so nothing about the header's own bytes says another
/// region owns the pages behind it.
#[test]
fn freeing_one_headers_region_leaves_a_siblings_payload_readable() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![11, 22, 33, 44]);

    let doomed = region(&mut heap);
    let survivor = region(&mut heap);
    let _ = materialize(&mut heap, &p, doomed);
    let live = header(materialize(&mut heap, &p, survivor));

    heap.decref_region(doomed);

    assert_eq!(
        live.bytecode(),
        &[11, 22, 33, 44],
        "the shared payload must survive a sibling header's region"
    );
}

/// A payload region is an ordinary counted region: when the last blueprint
/// packed into it dies, the cache drops its reference and the region frees.
/// Nothing about a code object needs a second reclamation mechanism.
#[test]
fn a_dead_blueprint_releases_its_payload_region() {
    let mut heap = FiberHeap::new();
    let baseline = heap.active_region_count();

    let header_region = region(&mut heap);
    {
        let p = proto(vec![1, 2, 3, 4]);
        let _ = materialize(&mut heap, &p, header_region);
    }
    heap.decref_region(header_region);
    heap.release_dead_template_payloads();

    assert_eq!(
        heap.active_region_count(),
        baseline,
        "a dead blueprint's payload region must be released, not held to teardown"
    );
}

/// A macro expansion reclaims its scratch by balancing the references a scan of
/// heap contents cannot explain, and the cache's reference is held in Rust,
/// where that scan cannot reach it. So a payload materialized while the scope is
/// open must be excluded from the reclaim, exactly as the process roots are.
///
/// The counter-factual is the reclaim taking that reference: the region survives
/// until the expansion's own headers die and is freed with them, while the cache
/// goes on naming it. The next header built from this blueprint then reads
/// recycled pages — one function's bytecode over another's constant pool, which
/// surfaces as a `LoadConst` indexing past the pool, arbitrarily far from here.
///
/// The transformer that gets there is ordinary: a macro whose template builds a
/// closure runs `MakeClosure` inside the scope, and the first `MakeClosure` for
/// a blueprint materializes its payload.
#[test]
fn a_payload_materialized_inside_a_macro_scope_survives_the_reclaim() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![1, 2, 3, 4, 5, 6, 7, 8]);

    crate::value::arena::begin_macro_scope(&mut heap);
    let scratch = region(&mut heap);
    let tv = materialize(&mut heap, &p, scratch);
    let payload =
        RuntimeRegion::new(heap.region_of_ptr(header(tv).bytecode().as_ptr() as *const ()))
            .expect("the payload lives in a real region");
    let generation = heap.region_generation(payload.get());
    crate::value::arena::reclaim_macro_scope(&mut heap);

    assert!(
        heap.region_rc(payload) > 0,
        "the cache still names this payload, so the scope must leave its \
         reference alone"
    );
    assert_eq!(
        heap.region_generation(payload.get()),
        generation,
        "a bumped generation means the region was freed and the id recycled"
    );
    assert_eq!(
        header(header_in(&mut heap, &p)).bytecode(),
        &[1, 2, 3, 4, 5, 6, 7, 8],
        "the cached payload must still read as the code it was built from"
    );
}

/// The exclusion covers every payload region, not the one the heap happens to
/// have opened last. Payloads are packed several to a region, so an expansion
/// reaches a fresh one exactly when the open region passes its size threshold —
/// which is why the failure is cumulative in a long run rather than a property
/// of any one macro.
///
/// The trap: a heap with no payload region yet opens one on its first
/// materialization, so a test that starts from a fresh heap cannot tell the
/// two cases apart. Here the first blueprint fills a region outside the scope,
/// and the expansion opens the second one inside it.
#[test]
fn a_payload_region_opened_inside_a_macro_scope_survives_the_reclaim() {
    let mut heap = FiberHeap::new();
    let bulky = proto(vec![0x5A; 300 * 1024]);
    let first = header_in(&mut heap, &bulky);
    let full =
        RuntimeRegion::new(heap.region_of_ptr(header(first).bytecode().as_ptr() as *const ()))
            .expect("the payload lives in a real region");

    crate::value::arena::begin_macro_scope(&mut heap);
    let scratch = region(&mut heap);
    let p = proto(vec![7, 7, 7, 7]);
    let tv = materialize(&mut heap, &p, scratch);
    let payload =
        RuntimeRegion::new(heap.region_of_ptr(header(tv).bytecode().as_ptr() as *const ()))
            .expect("the payload lives in a real region");
    assert_ne!(
        payload, full,
        "the first blueprint must fill its region, so this payload opens a \
         fresh one inside the scope"
    );
    crate::value::arena::reclaim_macro_scope(&mut heap);

    assert!(
        heap.region_rc(payload) > 0,
        "a payload region opened inside the scope is held by the cache like \
         any other"
    );
    assert!(
        heap.region_rc(full) > 0,
        "the region opened before the scope was never the scope's to reclaim"
    );
}

/// Mint `rounds` blueprints, each one dead before the next is minted, and
/// answer the addresses they occupied.
///
/// The trap: dropping the `Rc` is not enough to kill a blueprint. The header
/// materialized from it holds its own strong reference, so the blueprint dies
/// only when the header's region is freed — which is why each round mints a
/// region of its own and frees it before the round ends.
///
/// Every blueprint is materialized, so every one leaves an entry behind, and
/// `rounds` stays under the sweep floor so no sweep runs inside the loop. That
/// is the window in which the cache holds nothing but dead blueprints' entries.
fn mint_and_drop_blueprints(heap: &mut FiberHeap, rounds: u8) -> Vec<usize> {
    (0..rounds)
        .map(|byte| {
            let holder = region(heap);
            let p = proto(vec![byte; 16]);
            let _ = materialize(heap, &p, holder);
            heap.decref_region(holder);
            assert_eq!(
                Rc::strong_count(&p),
                1,
                "the header's reference must be gone, so dropping this one \
                 leaves the block free for the next round to land in"
            );
            Rc::as_ptr(&p) as usize
        })
        .collect()
}

/// The cache is keyed by blueprint address, and the allocator hands a freed
/// block back to the next allocation of the same layout. What keeps the key
/// sound is the entry's `Weak`: it holds the blueprint's allocation after the
/// last strong reference goes, so a cached address stays reserved and no live
/// blueprint can be at it.
///
/// The trap this replaces: a test that mints one blueprint, drops it, mints a
/// second and calls the case covered. It never reached the case, because the
/// case cannot happen — and it read as if it usually did.
///
/// The counter-factual is a key that does not pin, a raw address or an id
/// drawn from a counter that wraps. The blueprints below would then collide,
/// and one would be handed the other's bytecode — silent, and surfacing only
/// as the wrong function running.
#[test]
fn a_cached_blueprints_address_is_never_handed_to_another() {
    let mut heap = FiberHeap::new();

    let mut addresses = mint_and_drop_blueprints(&mut heap, 64);
    let minted = addresses.len();
    addresses.sort_unstable();
    addresses.dedup();

    assert_eq!(
        addresses.len(),
        minted,
        "each dead blueprint's entry holds its allocation, so none of the {minted} \
         addresses can be handed to a later blueprint"
    );
}

/// A payload region counts the entries naming it, and that count decides when
/// the region is released. Nothing else reads it, so the invariant is the only
/// place it can be seen: one claim per entry, however many blueprints have
/// lived and died.
///
/// The counter-factual is any motion of the count that is not an entry
/// arriving or leaving. It costs nothing a header reads — the code stays
/// correct — and shows up only as a region whose count can no longer reach
/// zero.
#[test]
fn every_entry_holds_one_claim_on_its_payload_region() {
    let mut heap = FiberHeap::new();

    mint_and_drop_blueprints(&mut heap, 64);

    let (entries, claims) = heap.template_payload_census();
    assert_eq!(
        claims, entries,
        "{claims} claims against {entries} entries; the two move together or \
         a payload region is held past its last blueprint"
    );

    heap.release_dead_template_payloads();
    let (entries, claims) = heap.template_payload_census();
    assert_eq!(entries, 0, "every blueprint above is dead");
    assert_eq!(claims, 0, "the sweep takes each entry's claim with it");
}

/// What the claim decides, at the scale the region is packed for: many
/// blueprints share one payload region, so the region is released only when
/// the sweep has removed the last of them.
#[test]
fn a_sweep_releases_the_region_every_blueprint_has_left() {
    let mut heap = FiberHeap::new();
    let baseline = heap.active_region_count();

    mint_and_drop_blueprints(&mut heap, 64);
    heap.release_dead_template_payloads();

    assert_eq!(
        heap.active_region_count(),
        baseline,
        "every blueprint is dead, so the payload region they shared must be \
         released by the sweep rather than held to teardown"
    );
}
