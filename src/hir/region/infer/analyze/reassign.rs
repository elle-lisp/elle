// audited: 2026-09-21
//! The mutable-reassign 1-slot-container gate: the questions asked of each
//! reassigned binding, and what a yes records.
//!
//! A reassigned mutable binding (top-level file-letrec or fn-local) is modeled
//! as a 1-slot container rather than given a static last-use `decref_point`,
//! which would mis-target whatever the slot holds at that program point.
//!
//! docs/impl/region/bindings.md

// `super` is `hir::region::infer::analyze`; `super::super` reaches the sibling
// `hir::regions` items (`RegionHolders`, `RegionInfo`, `Binding`, …) the
// original block saw through `use super::*` at the analyze root.
use super::super::holders::RegionHolders;
use super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::region::CellContainer;

// The two facts the gate reads but does not itself decide, each its own subject:
// the forwarding chain one reassigned name becomes, and the feeder a store
// consumes.
mod chain;
mod feeder;

pub(super) use chain::Reassigns;
use feeder::Feeders;

/// One fn-local reassigned binding's answers to the gate questions, kept apart
/// from the application because the whole-chain rule needs every link's answer
/// before any link may act on its own.
struct LocalVerdict {
    /// Every region the model PINS to a store site — the values this chain
    /// stores — has no other holder (after the forwarding fold). The pin moves a
    /// producer release EARLIER, so a second name reading the value would be
    /// left holding a freed one. Cleared for every link of a chain any link
    /// fails.
    stored_sole: bool,
    /// Every region the model would SUPPRESS — the init, which the cell takes
    /// uncounted — has no other holder, so the donation is available. Where it
    /// is not, the cell counts its init instead and suppresses nothing
    /// (docs/impl/region/bindings.md § "What the cell donates it must hold
    /// alone; what it counts it need not").
    donates_init: bool,
    /// Where the chain's binder stores the init value — the one position the
    /// counted-init retain can take. `None` leaves donate-or-refuse.
    init_site: Option<HirId>,
}

impl LocalVerdict {
    /// The binding takes the full container model: drop-on-overwrite for each
    /// displaced prior, and a content drop for the final one.
    ///
    /// Whether the binding's content is RETURNED is not part of the question. A
    /// `Return` mints the caller's reference (`lower_return`), which the callee
    /// did not hold a moment earlier, so it claims nothing the cell holds; and
    /// the cell's own reference is the counted store's. The order that keeps the
    /// pair exact is the lowerer's: the mint precedes the `Return` node's own
    /// releases, and the content drop is one of them
    /// (docs/impl/region/bindings.md § "Returned fn-local reassigned mutables —
    /// the return claims the MINT's reference, not the cell's").
    fn is_cell(&self) -> bool {
        self.takes_model()
    }

    /// The binding takes the model at all. The init's claim must be discharged
    /// one way or the other: donated (its ordinary decref suppressed, released
    /// by drop-on-overwrite) or counted at the chain source's binder.
    fn takes_model(&self) -> bool {
        self.stored_sole && (self.donates_init || self.init_site.is_some())
    }
}

/// Apply the 1-slot-container model for reassigned mutable bindings, recording
/// drop-on-overwrite / donation sites and decref suppressions into `info`.
pub(super) fn apply_reassign_containers(
    info: &mut RegionInfo,
    arena: &BindingArena,
    du: &DefUseBuilder,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    reassigns: &Reassigns,
    escape_info: &crate::hir::EscapeInfo,
    pd: &super::super::postdom::PostDom,
) {
    let Reassigns {
        top_level: top_level_reassigns,
        local: local_reassigns,
        binder_init_sites,
        ..
    } = reassigns;
    // A holder is a real alias only if it is a USER binding that is READ:
    // exclude the write-only `__file_expr_N` statement wrapper an assign result
    // flows into (never read), and — via the shared index — the synthetic ANF
    // producer temp `(let [_t e] _t)` (read once, same value flow). Otherwise
    // every reassigned binding looks aliased and the model never applies.
    // `RegionHolders` applies the universal synthetic exclusion on top of
    // whatever eligibility filter it is handed.
    let is_read = |b: Binding| -> bool { du.uses.get(&b).is_some_and(|u| !u.is_empty()) };
    // A read binding that merely carries a value INTO a store is the store's
    // FEEDER, not a second holder of that value: the store-site pin lands where
    // the feeder dies, so the sole-held question has nothing to protect
    // (docs/impl/region/bindings.md § "A name the store consumes is not a second
    // holder of the value"). The everyday one is the element name `each` binds,
    // whose exclusion is what lets a walk's cell take the model at all.
    let feeders = Feeders::collect(du, info, inference_binding_regions, reassigns, pd);
    let holds = |b: Binding| -> bool { is_read(b) && !feeders.contains(b) };
    let next = reassigns.forwarding_edges();
    let mut region_holders = RegionHolders::with_aliases(
        inference_binding_regions,
        arena,
        &holds,
        Reassigns::forwarded_init_aliases(&next),
    );
    for (b, stores) in top_level_reassigns.iter().chain(local_reassigns.iter()) {
        if is_read(*b) {
            region_holders.add(*b, arena, stores.value_regions());
        }
    }
    let sole_held = |b: Binding, r: Region| -> bool { region_holders.sole_held(b, r) };
    // ── Returned-value exclusion ────────────────────────────────────────
    // (docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
    // containers".) The container model claims each value region's single
    // compiler-owned reference for the cell (released by drop-on-overwrite
    // or frame/scope teardown) and suppresses the region's ordinary decref.
    // A value that ALSO flows to a function's tail/return is claimed a SECOND
    // time by the return's `IncrefValueRegion` (the mint-at-return
    // convention) — two static claims on one cell, so the gate must refuse
    // and fall back to the unsuppressed baseline (over-keeping, never
    // mis-freeing). The "is this cell's value returned" question is answered
    // per-binding by `EscapeInfo`'s return facet (`binding_escapes_via_return`,
    // below), not by projecting a region set.
    //
    // Deliberately NOT refused — runtime-counted escapes are compatible
    // with the model and must keep the gate (the boundary is pinned by
    // `reassign_gate_keeps_*` tests; refusing them regresses the
    // mutable-reassign pins straight back to UAFs):
    //   - mutable-container stores (push/put funnels incref at runtime),
    //   - capture into a closure env (alloc-scan incref + free cascade),
    //   - opaque-call arg cliques (mutual may-store edges; a real store
    //     increfs at runtime, and the edge's compile-time IncrefRegion is
    //     balanced by the target's free-time cascade),
    //   - value-succession into the binding's own next value
    //     (`(assign acc (pair i acc))` — alloc-scan counted).
    // Like sole_held, the check is per-binding, all-or-nothing.
    for (b, stores) in top_level_reassigns {
        let regions = stores.value_region_set();
        let init_regions = inference_binding_regions
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        // Backstop (docs/impl/region/bindings.md "a mutated slot is not a
        // release route"), recorded UNCONDITIONALLY — before the
        // sole/returned gate. A top-level (file-letrec) reassigned binding's
        // slot is overwritten over time, so a value-routed release
        // (`LoadLocal slot` + `DecrefValueRegion`) of ANY region it holds —
        // init OR assign value — at that region's `decref_point` loads
        // whatever the slot holds THEN (a later, live value) and frees it,
        // not the region intended (the no-alias corruption UAF,
        // region-mutable-reassign-flow facet 3: a deref-cell read is solved
        // to the cell's init region, pushing the init's decref to the read's
        // last use and routing it through the now-reassigned cell slot). When
        // the gate SUCCEEDS these are already in `suppressed_decref_regions`;
        // when it FAILS the lowerer skips the value route for any region here.
        // The final never-overwritten value is freed by file-letrec frame
        // teardown (its region lives in the frame region, cascade-freed), not
        // by a slot route, so skipping ALL of them only over-keeps until
        // teardown — never a leak, never a mis-free. (Fn-local reassigns are
        // NOT recorded: their final value's release IS a legitimate
        // scope-exit slot route, and the scope-based solver shares regions, so
        // skipping there leaks an aliased value — region-tailcall-arg-transfer.)
        for &r in init_regions.iter().chain(regions.iter()) {
            info.mutated_binding_value_regions.insert(r);
        }
        // **Not-returned check reads `EscapeInfo`** (the one authoritative
        // escape analysis). The gate refuses the container model for a *returned*
        // value (the return transfers the value's reference to the caller, which
        // the cell also claims — two static owners) but keeps it for a value that
        // merely stores into a container or is captured (runtime-counted). That
        // is exactly the *return facet*: `binding_escapes_via_return`.
        //
        // Read per-binding (atom-level), NOT by projecting a returned-region set
        // onto the cell's regions — `binding_source_regions` is "where the value
        // points", not "where it lives", so that projection is unsound. Where the
        // return facet is precise about a cell that merely *points* at a returned
        // region without itself flowing to a tail, the value is genuinely not
        // returned, so applying the model is correct; and such shapes are
        // independently sole-held-refused (the "refused twice over" invariant
        // below), so the gate *outcome* is unchanged.
        //
        // Guarded by "the cell carries a heap region": the return facet is
        // value-flow, so an immediate-valued cell read in tail position is
        // "returned", but it carries no reference to transfer — the region model
        // never refused on one, and there is no decref to suppress regardless.
        let has_heap_region = !init_regions.is_empty() || !regions.is_empty();
        let returned = has_heap_region && escape_info.binding_escapes_via_return(*b);
        let all_sole = !returned
            && init_regions
                .iter()
                .chain(regions.iter())
                .all(|&r| sole_held(*b, r));
        if !all_sole {
            continue;
        }
        // Module-scope container: the producer's reference is donated to the
        // cell (its ordinary decref is suppressed below), so the lowerer's
        // drop-on-overwrite is its sole release and NO incref-on-store is added.
        // `donated_overwrite_sites` carries that to `lower_assign` — without it
        // an unbalanced incref holds every displaced prior to teardown
        // (docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
        // containers"). The fn-local loop deliberately does NOT mark its
        // sites here (its assign-value decref is kept, balancing the incref).
        //
        // CALL-RESULT content is excluded from the donation, exactly as in the
        // fn-local branch below. A call result carries a SECOND compile-time name
        // for the same runtime value — the opaque placeholder region the lowerer
        // releases by value through the ANF temp's slot (Rule 2's bound-result
        // shape) — and the suppression below reaches only the value's own source
        // regions, never that placeholder. So the placeholder release still fires
        // and consumes the callee's single returned reference; donating on top of
        // it leaves the cell holding a freed value (`region-hof-tail-return-uaf.lisp`,
        // whose callee returns a frozen array through a `cond` arm). Taking the
        // counted store instead balances: store incref + placeholder release = the
        // cell's one reference, dropped at the next overwrite.
        let donates = !regions.iter().any(|r| info.call_result_regions.contains(r));
        for s in stores.sites() {
            info.drop_on_overwrite_sites.insert(s);
            if donates {
                info.donated_overwrite_sites.insert(s);
            }
        }
        // Suppress the compiler's ordinary decrefs for BOTH the init region
        // and every assign-value region. Each of those values ALSO carries a
        // static `DecrefRegion` (its `(let [_t v] _t)` ANF scope region) that
        // is its single owning demise; the value-based `DecrefValueRegion`
        // here would be a SECOND decref of the same region (the read-time
        // double-free witnessed in the rc trace). The cell's own reference is
        // supplied by `lower_assign`'s incref-on-store and released by
        // drop-on-overwrite (priors) or frame teardown (final value).
        for &r in init_regions.iter().chain(regions.iter()) {
            info.suppressed_decref_regions.insert(r);
        }
    }

    // ── Fn-local (in-lambda) reassigned mutables ───────────────────────────
    // Same 1-slot-container model as the top-level loop above — the cell takes a
    // COUNTED reference via `lower_assign`'s incref-on-store, released by
    // drop-on-overwrite for each displaced prior. ONE difference: a fn-local
    // cell's final content is NOT a program-lifetime root (a module-scope cell's
    // is, freed by the file-letrec frame teardown), so the cell needs a second
    // release channel of its own — the CONTENT DROP at the cell's scope demise,
    // recorded in `cell_containers` and emitted by the lowerer at the enclosing
    // scope node's exit. The producer's separate claim on each stored value is
    // dead once the cell holds its own reference, so it is pinned to the store
    // site (`decref::populate_decref_points` reads `cell_containers` for both).
    // Two references, two channels each: no release does double duty, so the
    // accounting holds for a cell written once and for one re-minted every
    // iteration of a loop alike.
    //
    // The gate is sole-held, asked per region over the half it decides (BOTH
    // not-returned and returned — see the split below and
    // docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
    // containers" § "Returned fn-local reassigned mutables"). Distinct
    // mechanism: a `@`-mutable PARAMETER (a captured cell the callee owns)
    // reassigned then moved into a tail call is released by the callee's own cell
    // `DecrefCellRegion`, and the tail move's borrowed-arg retain must order ahead
    // of that release's cascade — enforced in `lower_call` (pinned by
    // region-mutable-reassign-param.lisp), not by this gate.
    //
    // The chains, and what each KEEPS the ordinary decref of: each link's own
    // assign-value regions, which are the producer releases pinned to the store
    // sites. A downstream link's source regions contain every upstream link's
    // (the `Loop` init copies them), so suppressing by one link's `regions` alone
    // would cancel an upstream link's producer releases and strand every value
    // that link displaced. Computed before the verdicts, because the split
    // between what the model PINS and what it SUPPRESSES is exactly `kept` vs
    // the rest, and each half answers a different question.
    let mut chains: HashMap<Binding, Vec<Binding>> = HashMap::new();
    for &b in local_reassigns.keys() {
        chains
            .entry(Reassigns::last_of_chain(&next, b))
            .or_default()
            .push(b);
    }
    let chain_kept: HashMap<Binding, Vec<Region>> = chains
        .values()
        .flat_map(|links| {
            let kept: Vec<Region> = links
                .iter()
                .flat_map(|b| local_reassigns[b].value_regions())
                .collect();
            links.iter().map(move |&b| (b, kept.clone()))
        })
        .collect();
    // Every VERSION of a chain, keyed by the link the fold resolves to — the
    // reassigned links plus the upstream source names that only forward. A
    // pre-loop version is never assigned, so `local_reassigns` does not name it,
    // yet it is the version whose binder stores the chain's init.
    let mut chain_versions: HashMap<Binding, Vec<Binding>> = HashMap::new();
    for &b in next.keys().chain(local_reassigns.keys()) {
        let last = Reassigns::last_of_chain(&next, b);
        let versions = chain_versions.entry(last).or_default();
        if !versions.contains(&b) {
            versions.push(b);
        }
    }

    // Pass 1: each binding's own answers. Recorded before any binding acts,
    // because a chain's links stand or fall together (pass 2).
    let mut verdicts: HashMap<Binding, LocalVerdict> = HashMap::new();
    for (b, stores) in local_reassigns {
        let regions = stores.value_region_set();
        // Record the binding so the lowerer can refuse a value-route decref +
        // nil-stamp that names this binding's stack slot. `allocate_slot` gives a
        // fn-local reassigned mutable its own never-reused slot that holds a live
        // value across its whole scope; a spurious immediate-valued assign region
        // (`(assign ii (%add ii 1))`) kept by the branch below would otherwise
        // nil-stamp that slot mid-loop and zero the counter
        // (region-capture-cell-loop-uaf.lisp under --wasm=full).
        info.reassigned_local_bindings.insert(*b);
        let binding_regs = inference_binding_regions
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        // The sole-held question, asked separately of the two halves it decides.
        // `kept` — the chain's stored values — is what the model PINS back to a
        // store site, and moving a producer release earlier is unsafe under a
        // second name. Everything else the binding holds is what the model would
        // SUPPRESS, which is only ever the donation's business: an aliased init
        // costs the donation, not the model (docs/impl/region/bindings.md § "What
        // the cell donates it must hold alone; what it counts it need not").
        let kept = chain_kept.get(b).map(|v| v.as_slice()).unwrap_or(&regions);
        let last = Reassigns::last_of_chain(&next, *b);
        verdicts.insert(
            *b,
            LocalVerdict {
                stored_sole: binding_regs
                    .iter()
                    .filter(|r| kept.contains(r))
                    .all(|&r| sole_held(*b, r)),
                donates_init: binding_regs
                    .iter()
                    .filter(|r| !kept.contains(r))
                    .all(|&r| sole_held(*b, r)),
                init_site: chain_versions
                    .get(&last)
                    .and_then(|vs| Reassigns::init_store_site(vs, binder_init_sites)),
            },
        );
    }

    // Pass 2: the whole-chain rule. A chain of forwarding edges hands ONE
    // reference from link to link, so exactly one link may release it — the one
    // holding it at its own overwrite, or the last link at its demise. A link the
    // gate refuses keeps the unsuppressed baseline, where each value's ordinary
    // decref releases the producer's reference, and the next link's
    // drop-on-overwrite would then release it a second time. So a chain is
    // admitted or declined whole (docs/impl/region/bindings.md § "A chain of
    // forwarding edges hands one reference along, so the fold follows it whole").
    //
    // Donation is a whole-chain answer for the same reason: the init region is
    // the one every link's source set shares, and the suppression is keyed by
    // REGION, so one link donating while another counts would suppress the
    // release the counting link's alias still needs.
    for links in chains.values() {
        if links.len() < 2 {
            continue;
        }
        let donates = links.iter().all(|b| verdicts[b].donates_init);
        for b in links {
            verdicts.get_mut(b).unwrap().donates_init = donates;
        }
        if !links.iter().all(|b| verdicts[b].is_cell()) {
            for b in links {
                verdicts.get_mut(b).unwrap().stored_sole = false;
            }
        }
    }

    // Pass 3: apply.
    for (b, stores) in local_reassigns {
        let regions = stores.value_region_set();
        let binding_regs = inference_binding_regions
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let LocalVerdict {
            donates_init,
            init_site,
            ..
        } = verdicts[b];
        let takes_model = verdicts[b].takes_model();
        // The cell's final content is handed to the next link, which releases it
        // at its own first overwrite (or at its own content drop). The chain rule
        // above makes that link a cell whenever this one is.
        let forwards_content = next.contains_key(b);
        let kept = chain_kept.get(b).map(|v| v.as_slice()).unwrap_or(&regions);

        if takes_model {
            // The cell's content dies at the overwrite (priors) and at the
            // cell's scope demise (the final value). The first overwrite is the
            // init value's owning demise, so drop-on-overwrite is its release
            // channel too. A binding whose content the frame RETURNS is no
            // different: the `Return`'s mint is the caller's reference and the
            // content drop, which the lowerer emits after that mint at the same
            // node, is the cell's (docs/impl/region/bindings.md § "Returned
            // fn-local reassigned mutables").
            for s in stores.sites() {
                info.drop_on_overwrite_sites.insert(s);
            }
            // The demise is seeded with the last store — the earliest point
            // that is after every write — and `decref::populate_decref_points`
            // moves it out to the cell's last read and past any loop the
            // cell is carried across, both of which need the structural
            // order this pass runs before. A FORWARDING link records the
            // container all the same — the store-site pins and the
            // hold-back from the binding chain are its business too — but
            // without the content drop the next link takes over.
            if let Some(seed) = stores.sites().last() {
                let cell = if forwards_content {
                    CellContainer::forwarding(stores.clone(), seed)
                } else {
                    CellContainer::new(stores.clone(), seed)
                };
                info.cell_stored_regions.extend(cell.stores.value_regions());
                info.cell_containers.insert(*b, cell);
            }
            // KEEP the CHAIN's assign-value regions' (`kept`) decrefs and
            // suppress every OTHER region the binding may hold
            // (`binding_regs \ kept` — the init region, and, for a binding
            // accumulated in a LOOP, the loop-carried binding region that
            // aliases whatever value the slot currently holds).
            //
            // The kept assign-value decref is the PRODUCER's release of each
            // stored value (pinned to the store site, where the cell's counted
            // reference takes over); the cell's own reference is released by
            // drop-on-overwrite and the content drop above. The init value is
            // donated — it is stored uncounted at the define, so suppressing its
            // ordinary decref leaves drop-on-overwrite (or the content drop, if
            // it is never displaced) as its one release.
            //
            // The loop-carried region is what the suppression is for when the
            // frame RETURNS the binding: a loop gives the binding its OWN
            // region (the slot that carries the accumulator across the back-edge)
            // that aliases the SAME runtime value as the reaching assign-value
            // region, so leaving both unsuppressed emits TWO value-route decrefs
            // of one reference at the Return — the second frees the caller's
            // minted reference before the caller's read (the
            // loop-reassigned-return double-free,
            // `region_capture_cell_string_accum_uaf`). A single-assign cell
            // coalesces its binding and assign-value regions
            // (`binding_regs == regions`), so this suppresses nothing there.
            //
            // All of that is the DONATION, and it is available only where the
            // cell is the init value's sole holder. Where a second name reads
            // that value, the cell takes a counted reference at the chain
            // source's binder instead — balanced by the same drop-on-overwrite
            // that balances every later store — and suppresses nothing, leaving
            // the alias the ordinary decref that releases the producer's
            // reference (docs/impl/region/bindings.md § "What the cell donates
            // it must hold alone; what it counts it need not").
            if donates_init {
                for &r in binding_regs {
                    if !kept.contains(&r) {
                        info.suppressed_decref_regions.insert(r);
                    }
                }
            } else if let Some(site) = init_site {
                info.counted_cell_init_sites.insert(site);
            }
        }
        // else: leave the unsuppressed baseline.
    }
}
