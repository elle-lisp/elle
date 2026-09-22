// audited: 2026-09-22
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
// consumes. The module-scope half is a third, because what it claims — the
// producer's own reference — is not what the fn-local half claims.
mod chain;
mod feeder;
mod toplevel;

pub(super) use chain::Reassigns;
use feeder::Feeders;

/// One fn-local reassigned binding's answers to the gate questions, kept apart
/// from the application because the whole-chain rule needs every link's answer
/// before any link may act on its own.
struct LocalVerdict {
    /// The whole-chain rule's veto (pass 2): a chain of forwarding edges hands
    /// one reference along, so its links are admitted or declined together, and
    /// a link whose own verdict fails declines every other link with it.
    chain_admitted: bool,
    /// Every region the model PINS to a store site — the values this chain
    /// stores — is stored once per binding of every name that holds it.
    ///
    /// This is all the pin asks of a second name. The pin is a maximum over the
    /// extensions the region already carries, so an alias's own reads move it
    /// later and refuse nothing; what it cannot survive is N stores against one
    /// producer reference, which a name bound outside the loop that stores it
    /// produces (docs/impl/region/bindings.md § "The store-site pin asks only
    /// that the store run once per binding of the name it reads").
    stored_pinnable: bool,
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
        self.chain_admitted
            && self.stored_pinnable
            && (self.donates_init || self.init_site.is_some())
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
    // FEEDER, not a second holder of that value: nothing reads it after the
    // store, so the donation's sole-held question has nothing to protect
    // (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
    // what it counts it need not"). The everyday one is the element name `each`
    // binds.
    let feeders = Feeders::collect(du, info, inference_binding_regions, reassigns, pd);
    let holds = |b: Binding| -> bool { is_read(b) && !feeders.contains(b) };
    // The STORE-SITE PIN's index carries the ONE holder it cannot survive: a name
    // that feeds a store running more than once per binding of it, whose single
    // producer reference the pin would release N times. Every other second name
    // refuses the pin nothing — the pin is a maximum over the extensions the
    // region already carries, an alias's own reads among them, so a later read
    // moves it later (docs/impl/region/bindings.md § "The store-site pin asks
    // only that the store run once per binding of the name it reads").
    let pins = |b: Binding| -> bool { is_read(b) && feeders.over_feeds(b) };
    let next = reassigns.forwarding_edges();
    let aliases = Reassigns::forwarded_init_aliases(&next);
    let mut region_holders =
        RegionHolders::with_aliases(inference_binding_regions, arena, &holds, aliases.clone());
    let mut pin_holders =
        RegionHolders::with_aliases(inference_binding_regions, arena, &pins, aliases);
    for (b, stores) in top_level_reassigns.iter().chain(local_reassigns.iter()) {
        if is_read(*b) {
            region_holders.add(*b, arena, stores.value_regions());
            pin_holders.add(*b, arena, stores.value_regions());
        }
    }
    let sole_held = |b: Binding, r: Region| -> bool { region_holders.sole_held(b, r) };
    let pin_sole = |b: Binding, r: Region| -> bool { pin_holders.sole_held(b, r) };
    // The module-scope half, whose cell ADOPTS the producer reference and whose
    // final content the file-letrec frame teardown frees.
    toplevel::apply_module_scope(
        info,
        top_level_reassigns,
        inference_binding_regions,
        escape_info,
        &sole_held,
    );

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
    // The gate is the INIT's discharge — donated where sole-held, counted at
    // the chain source's binder otherwise (see the verdicts below and
    // docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
    // containers"). Whether the content is returned decides nothing
    // (§ "Returned fn-local reassigned mutables"). Distinct
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
        // The sole-held question is the DONATION's alone. `kept` — the chain's
        // stored values — is what the model PINS back to a store site, and the
        // pin is a maximum over every extension the region carries, an alias's
        // own reads included, so a second name refuses nothing there
        // (docs/impl/region/bindings.md § "The store-site pin asks only that the
        // store run once per binding of the name it reads"). What the model
        // would SUPPRESS, which is the donation's business: an aliased init
        // costs the donation, not the model (§ "What the cell donates it must
        // hold alone; what it counts it need not"). A binding-source region
        // that is ALSO stored is the one shape the two questions cannot split —
        // the suppression loop skips `kept`, so a donation granted over the
        // overlap would leave the define's uncounted store with no reference
        // for drop-on-overwrite to release — and an aliased overlap therefore
        // takes the counted-init route instead of donating.
        let kept = chain_kept.get(b).map(|v| v.as_slice()).unwrap_or(&regions);
        let last = Reassigns::last_of_chain(&next, *b);
        let overlap_aliased = binding_regs
            .iter()
            .filter(|r| kept.contains(r))
            .any(|&r| !sole_held(*b, r));
        verdicts.insert(
            *b,
            LocalVerdict {
                chain_admitted: true,
                stored_pinnable: binding_regs
                    .iter()
                    .filter(|r| kept.contains(r))
                    .all(|&r| pin_sole(*b, r)),
                donates_init: !overlap_aliased
                    && binding_regs
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
                verdicts.get_mut(b).unwrap().chain_admitted = false;
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
