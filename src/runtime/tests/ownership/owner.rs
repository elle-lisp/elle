// audited: 2026-10-11
//! Owner-node cuts end to end: the capture-back-edge cycle, the transferred returned cycle, and their parks.
//!
//! docs/impl/region/owner.md
//! docs/impl/region/adopt.md

use super::*;

/// End-to-end reclamation of the **capture-back-edge cycle** — the activation-owner cut
/// (docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge SCC"; inference
/// pin `region::infer::tests::adopt::activation_adopts_capture_back_edge_scc`). A Fresh container
/// `root` holds `m` (store `root ⊇ m`), `m` holds a closure `c` (store `m ⊇ c`), and `c`
/// captures `m` back (capture `c ⊇ m`) — the m↔c cycle through a closure env. No REGION root
/// can own it: `m` is captured, so its `decref_point` over-extends past the closure while its
/// own `DecrefValueRegion` stays live — the owner-aware lifetime obligation refuses (the
/// refusal `adopt_edges_refuses_captured_store_member_on_lifetime` pins; running 50×
/// panic-clean guards the over-free a region-root adopt of `m` would cause). The ACTIVATION owns it
/// instead: both members are `AdoptIntoActivation`'d at the SCC's enclosing scope, their own
/// decrefs suppressed, and the activation's completion release subtree-drops the cycle —
/// interior m↔c references reclaiming with the set.
///
/// The leaking discriminator is the built-in counterfactual: per-region RC cannot collect
/// the m↔c cycle (region/rules.md Rule 8), so a bounded reading beside a leaking
/// discriminator proves the cut, not the shape, reclaims it. The interior store is a native
/// funnel `Call` whose containment is funnel-recovered `containment_edges` (the
/// value-resolved adopt needs no store site).
#[test]
fn region_ownership_capture_back_edge_cycle_reclaims() {
    // root ⊇ m (store), m ⊇ c (store), c ⊇ m (capture) — the m↔c cycle through a
    // closure env, the cycle no region root can own (region/owner.md § "Owner nodes" —
    // "The capture-back-edge SCC"). Both members are `AdoptIntoActivation`'d and the
    // activation's completion release subtree-drops the cycle; a broken adopt or a
    // doubled member release trips a debug generation/decref assert, so completing
    // panic-clean over 50 runs is itself the soundness half of the pin.
    const SUBJECT: &str = "(begin (let [root (@array) m (@array)] \
                            (let [c (fn [] (length m))] \
                              (begin (%array-push m c) (c) (%array-push root m) nil))) \
                          nil)";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the capture-back-edge cycle must be reclaimed by the activation owner node's \
         completion release — per-run live-region growth {on} must be <= 0 (the \
         discriminator leaks {leak})",
    );
}

/// The capture-back-edge SCC with a park INSIDE the adopt scope, on the two
/// in-process abandonment routes — the park split end-to-end
/// (docs/impl/region/owner.md § "Owner nodes" — "The park split"; inference pin
/// `region::infer::tests::adopt::activation_adopt_sites_ahead_of_park`; the
/// abort route and both heap dimensions ride the `adopt-park-*` oracle family).
/// The early adopt runs after the members' allocations and before the yield, so
/// the parked frame carries the members in its activation owner node; the
/// handle drop's free-path discharge and the cancel's terminal teardown then
/// free node + members. The trap this gauges: the scope-exit adopt alone sits
/// PAST the yield, so a route that abandons the park would strand the whole
/// SCC (~2 regions per op) — suppressed members that no release table names
/// and no node ever adopted.
#[test]
fn region_ownership_parked_capture_back_edge_scc_reclaims_on_abandonment() {
    const PRELUDE: &str = "(def body (fn [] (let [root (@array) m (@array)] \
        (let [c (fn [] (length m))] \
          (begin (%array-push m c) (c) (%array-push root m) \
                 (emit :yield 0) (length root))))))";
    let leak = mid_run_discriminator(Runtime::without_stdlib(), "arena/region-count");
    assert!(
        leak > 150,
        "gauge live: the self-referential accumulator must grow (~200); got {leak}",
    );
    let routes = [
        (
            "drop",
            "(let [f (fiber/new body 2)] (begin (fiber/resume f) nil))",
        ),
        (
            "cancel",
            "(let [f (fiber/new body 2)] \
               (begin (fiber/resume f) (fiber/cancel f :dead) nil))",
        ),
    ];
    for (route, body) in routes {
        let growth = mid_run_growth(
            Runtime::without_stdlib(),
            PRELUDE,
            body,
            "arena/region-count",
        );
        assert!(
            growth < 50,
            "a park inside the adopt scope must reclaim the SCC on the {route} \
             route — per-op region growth must be near zero over 200 iterations, \
             got {growth} (the discriminator grows {leak})",
        );
    }
}

/// End-to-end reclamation of the **transferred returned cycle** — the
/// consuming-activation owner cut (docs/impl/region/owner.md § "Owner nodes" —
/// "The transferred returned subtree"; inference pin
/// `region::infer::tests::adopt::transfer_adopts_returned_cycle_to_consumer`). A
/// producer `mk` builds an a↔b cycle and returns its root; the top-level
/// consumer discards it. No region root can own it (the root crosses the
/// return frontier) and per-region RC cannot collect the cycle, so RC alone
/// leaks it per call. The producer's interior adopt hangs `b` under the
/// returned root and the consumer's release is replaced by
/// `AdoptIntoActivation`, so the activation's completion release set-drops the
/// whole cycle.
///
/// The leaking discriminator is the built-in counterfactual beside the bounded
/// reading. The interior store is a native funnel `Call` — funnel-recovered
/// containment whose adopt is keyed at the funnel call site (the value-resolved
/// adopt needs no store opcode).
#[test]
fn region_ownership_reclaims_returned_cycle_across_calls() {
    // A producer `mk` builds an a↔b cycle and returns its root; the top-level consumer
    // discards it. No region root can own it (the root crosses the return frontier) and
    // per-region RC cannot collect the cycle, so the transfer cut hangs `b` under the
    // returned root and replaces the consumer's release with `AdoptIntoActivation`; the
    // activation's completion release set-drops the whole cycle. A broken adopt or a
    // doubled release trips a debug generation/decref assert, so 50 panic-clean runs
    // are the soundness half of the pin.
    const SUBJECT: &str = "(def mk (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a)))) \
                   (mk) \
                   (mk) \
                   nil";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the returned cycle must be reclaimed by the consuming activation's owner-node \
         release — per-run live-region growth {on} must be <= 0 (the discriminator \
         leaks {leak})",
    );
}

/// The **fiber face** of the transfer cut: a silent fiber body's terminal value
/// is the returned cycle, handed across the fiber frontier by the completing
/// resume and discarded. The fiber machinery balances its own retains — the
/// cycle is what remains without the cut; the resume result's release is
/// replaced by `AdoptIntoActivation` and the consuming activation's completion
/// reclaims it.
#[test]
fn region_ownership_reclaims_fiber_terminal_cycle() {
    // A silent fiber body's terminal value is the returned a↔b cycle, handed across the
    // fiber frontier by the completing resume and discarded. The resume result's release
    // is replaced by `AdoptIntoActivation` and the consuming activation's completion
    // reclaims it.
    const SUBJECT: &str = "(begin (let [f (fiber/new (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a))) 1)] \
                     (begin (fiber/resume f) nil)) \
                   nil)";
    let leak = leak_discriminator();
    let on = steady_region_growth(SUBJECT);
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run region growth \
         {leak}); if 0 the gauge is dead and the bounded assertion below is vacuous",
    );
    assert!(
        on <= 0,
        "the fiber-terminal cycle must be reclaimed at the consuming activation's \
         completion — per-run growth {on} must be <= 0 (the discriminator leaks {leak})",
    );
}

/// The transfer adopt **rides parks and the fiber teardown**, exercised
/// end-to-end by production-emitted adopts. The consumer is a FIBER BODY that
/// calls the producer, yields (parking its activation node with the adopted
/// cycle), and either completes (the resumed body's clean break frees node +
/// members) or is hard-killed mid-park (`fiber/cancel` → the terminal teardown
/// frees the parked node's members).
#[test]
fn region_ownership_transfer_adopt_rides_parks_and_fiber_teardown() {
    // This is a SOUNDNESS pin for the park/resume/cancel/teardown wiring: the transfer
    // adopt + owner node must ride the park, restore, and terminal teardown WITHOUT
    // double-freeing or dangling — a broken adopt or a doubled member release trips a
    // debug generation/decref assert, so completing 50× panic-clean is the pin. The
    // amount reclaimed is pinned elsewhere: the hand-emitted owner-node tests read it
    // through generation bumps, and `region_ownership_reclaims_fiber_terminal_cycle`
    // pins the growth of the non-parked shape.

    // Drained to completion: two cycles adopted into the body's activation node, a yield
    // parking the node between them; the resumed body's completion frees node + members.
    let complete = "(def mk (fn [] (let [a (@array) b (@array)] \
                      (begin (%array-push a b) (%array-push b a) a)))) \
                    (let [f (fiber/new (fn [] (begin (mk) (emit :yield 0) (mk) nil)) 2)] \
                      (begin (fiber/resume f) (fiber/resume f) nil)) \
                    nil";
    let _ = steady_region_growth(complete);

    // Hard-killed mid-park: the first cycle is adopted, the body parks at the yield, and
    // `fiber/cancel` tears the fiber down — the kill must free the parked activation
    // node's members (the second cycle is never built) without double-freeing.
    let cancel = "(def mk (fn [] (let [a (@array) b (@array)] \
                    (begin (%array-push a b) (%array-push b a) a)))) \
                  (let [f (fiber/new (fn [] (begin (mk) (emit :yield 0) (mk) nil)) 2)] \
                    (begin (fiber/resume f) (fiber/cancel f :dead) nil)) \
                  nil";
    let _ = steady_region_growth(cancel);
}

/// VM≡JIT parity for the transfer cut: the producer's interior `AdoptRegion`
/// and the consumer's `AdoptIntoActivation` + owner-node completion release all
/// run through compiled code. The consumer wrapper carries no `MakeClosure`
/// (the producer is a top-level def), so it JIT-compiles; `jit_compiled` guards
/// a vacuous reading exactly as the pins in `jit.rs` do.
#[cfg(feature = "jit")]
#[test]
fn region_ownership_reclaims_returned_cycle_under_jit() {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;

    fn growth() -> (i64, bool) {
        let mut rt = Runtime::without_stdlib();
        rt.vm().runtime_config.jit = JitPolicy::Eager;
        let src = "(def mk (fn [] (let [a (@array) b (@array)] \
                     (begin (%array-push a b) (%array-push b a) a)))) \
                   ((fn [] (begin (mk) nil))) \
                   nil";
        let prog = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(src, symbols, cctx, "<embed>")
                .expect("compiles")
                .0
        };
        {
            let (vm, _symbols, cctx) = rt.parts();
            let v = vm
                .execute_scheduled(&prog, cctx)
                .expect("runs (submits the JIT task)");
            assert!(v.is_nil());
        }
        rt.vm().drain_jit_pending();
        let jit_compiled = !rt.vm().jit_cache.is_empty();
        {
            let (vm, _symbols, cctx) = rt.parts();
            let v = vm.execute_scheduled(&prog, cctx).expect("runs");
            assert!(v.is_nil());
        }
        let baseline = rt.heap().active_region_count() as i64;
        for _ in 0..50 {
            let (vm, _symbols, cctx) = rt.parts();
            let v = vm.execute_scheduled(&prog, cctx).expect("runs");
            assert!(v.is_nil());
        }
        (
            rt.heap().active_region_count() as i64 - baseline,
            jit_compiled,
        )
    }

    let (on, jit_compiled) = growth();
    assert!(
        jit_compiled,
        "the consumer wrapper must JIT-compile — an empty jit_cache means a worker \
         died (e.g. on a missing translate arm)",
    );
    let leak = leak_discriminator();
    assert!(
        leak > 0,
        "gauge live: the refused-cycle discriminator must leak (per-run growth {leak})",
    );
    assert!(
        on <= 0,
        "the JIT-compiled consumer must reclaim the returned cycle — per-run growth \
         {on} must be <= 0 (the discriminator leaks {leak})",
    );
}

/// The consumer-facing adopt channel is IDEMPOTENT on an already-Owned child:
/// delivering one region to `AdoptIntoActivation` twice within an activation (a
/// masked-`:error` fiber restarted after handing out the same payload) leaves
/// it owned by the first adopt instead of tripping `adopt_region`'s one-owner
/// assert, and the completion release frees it exactly once.
#[test]
fn adopt_into_activation_absorbs_redelivery() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the same member twice, then return — the second adopt
        // must be a structural no-op (the debug one-owner assert would
        // otherwise detonate mid-loop).
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        // The unit lives until the body completes, and goes with this cycle.
        let unit = crate::value::CodeBuilder::from_bytecode(bc).unit(unsafe { &mut *heap_ptr });

        let result = vm.execute_bytecode_saving_stack(&unit.entry().code(), &Rc::new(vec![]));
        assert!(
            result.bits.is_empty(),
            "the double-adopt body completes normally"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the twice-delivered member must be freed exactly once, by the node's \
             completion release (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each activation's completion — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// Boundary pin for the capture-adopt admission (region/adopt.md § "The capture
/// adopt"): the nested-closure-captures-its-encloser family — the only shape family
/// whose owner-capture is an UPVALUE and which external uniqueness admits — must NOT be
/// over-claimed by the forest's capture adopt. The nested closure's region is minted
/// per CALL of the enclosing closure, so adopting the longer-lived member would free it
/// under the encloser's still-live env reference and re-adopt an already-Owned region on
/// the next construction; the lifetime obligation refuses the shape by construction (the
/// forwarding capture pins the member's tight last-use at/past the enclosing lambda node,
/// which post-dates the nested root's in-body drop in post-order). The member then stays
/// on the ordinary per-region-RC baseline.
///
/// This is a SOUNDNESS pin, over a body that constructs the nested closure TWICE per run
/// (`(e true)` recurses once through `(e false)`): running it 50× must be PANIC-CLEAN —
/// an over-admission would emit the adopt at o's construction, and the second
/// construction would re-adopt the already-Owned member, tripping the one-owner debug
/// assert (or a generation panic). Its region growth is whatever per-region RC yields
/// (`%pair`/`%first` are inline intrinsics) and is NOT this pin's concern — the
/// reclamation cuts are pinned by the `reclaims_*` tests above.
#[test]
fn upvalue_capture_family_runs_sound() {
    const FAMILY: &str = "(begin (let [m (%pair 1 2)] \
        (letrec [e (fn [k] (let [o (fn [] (if k (e false) (%first m)))] (o)))] \
          (begin (e true) nil))) \
      nil)";
    // 50 panic-clean runs — an over-admission double-free would detonate the
    // debug one-owner/generation asserts here.
    let _ = steady_region_growth(FAMILY);
}
