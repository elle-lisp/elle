// audited: 2026-09-22
//! What branch compensation knows about a region before it looks at any arm.
//!
//! Where it is used, where it is born, who holds it, and what taints its route.
//!
//! docs/impl/region/mechanism.md

use super::super::*;
use crate::hir::region::Region;

/// The per-region tables both compensation routes read, gathered in one pass
/// over the holder bindings and the allocation map.
pub(super) struct Premises {
    /// Region → its use HirIds, unioned over every holder binding so an aliased
    /// region sees all uses. A region with no recorded use cannot be analyzed —
    /// nothing says where it dies — and the caller skips it.
    pub(super) uses: HashMap<Region, Vec<HirId>>,
    /// Region → the HirIds the **live-in** premise is asked of: its allocation
    /// sites, plus the def site of the binder its release would ROUTE through.
    pub(super) anchors: HashMap<Region, Vec<HirId>>,
    /// Region → its allocation sites alone, for the loop-invariant guard.
    pub(super) allocs: HashMap<Region, Vec<HirId>>,
    /// How many distinct holder bindings name each region. The `tail`
    /// value-route releases through `region_to_slot[r]` — ONE slot — so a region
    /// several bindings name has several slots but one entry, and the per-arm
    /// load could target the wrong one. `tail` is restricted to single-holder
    /// regions, where the slot is unambiguous.
    pub(super) holder_count: HashMap<Region, u32>,
    /// Region → the uncounted opcode reads (`%get`/`%first`/`%rest`) that borrow
    /// out of it. Such a read hands back a value living inside the container and
    /// raises no count on it, so the container's release must post-date the
    /// READER, not the read (`analyze/decref.rs`, `uncounted_read_sites`). The
    /// global `decref_point` already carries that extension; the env-cell `tail`
    /// route carries it per arm, having no same-node retain to prove the borrow
    /// counted.
    pub(super) reads: HashMap<Region, Vec<HirId>>,
    /// Regions no local-slot value route may free.
    ///
    /// A region held by a MUTATED (reassigned) or CAPTURED binding cannot be
    /// freed by a local-slot value route: a reassigned slot is repointed over
    /// time (the value route loads whatever it holds NOW, freeing a live later
    /// value — the "mutated slot is not a release route" UAF,
    /// region-mutable-reassign-param), and a captured value is held cross-region
    /// by the closure env (freeing it via the local slot dangles the env
    /// reference). Their release is owned by the store / capture-cell path, never
    /// this one. The capture fact is the region forest's own reachability
    /// question, read from the region capture-graph
    /// (`super::super::escape::captured_bindings`), never the lexical proxy
    /// `is_captured` the solver is locked out of; mutation stays a direct
    /// structural read.
    ///
    /// Both taints are claims about a release routed through the holder's SLOT,
    /// so neither reaches an env cell, whose release names the cell BOX instead —
    /// a box `populate_env` mints once per activation, that an `assign` never
    /// repoints (it writes the cell's content), and that a capturer holds by the
    /// funnel's counted `closure ⊇ cell` edge rather than through this frame's
    /// slot (docs/impl/region/mechanism.md § "A compensating release of an env
    /// cell names the box, not the holder's slot").
    pub(super) tainted: std::collections::HashSet<Region>,
}

impl Premises {
    pub(super) fn collect(
        hir: &Hir,
        info: &RegionInfo,
        du: &DefUseBuilder,
        arena: &BindingArena,
        binding_regions: &HashMap<Binding, Vec<Region>>,
        binder_init_sites: &HashMap<Binding, Option<HirId>>,
    ) -> Self {
        let mut uses: HashMap<Region, Vec<HirId>> = HashMap::new();
        let mut anchors: HashMap<Region, Vec<HirId>> = HashMap::new();
        let mut holder_count: HashMap<Region, u32> = HashMap::new();
        let captured = super::super::escape::captured_bindings(hir);
        let mut tainted: std::collections::HashSet<Region> =
            super::super::escape::mutated_route_regions(
                arena,
                info,
                binding_regions,
                binder_init_sites,
            )
            .into_iter()
            .collect();
        for (b, regions) in binding_regions {
            let b_uses = du.uses.get(b);
            let def = du.def_site.get(b);
            let unsafe_holder = captured.contains(b);
            // A holder's DEF site anchors the live-in premise only where that
            // holder could be the release's ROUTE — the slot a value-routed
            // release loads is the allocating binder's, so only that binder's def
            // says where the slot is written (docs/impl/region/mechanism.md § "A
            // region's release route belongs to ONE binding"). An alias, a
            // functionalization version, or a phi merely names a value born
            // elsewhere; anchoring its def would read the value as born inside
            // whatever arm the naming sits in. An env CELL's release names the box
            // at the holder's env index rather than a value slot, so the holder's
            // def is exactly where its box is minted and keeps its anchor.
            let route_regions: Vec<Region> = match super::super::escape::binder_route(
                *b,
                arena,
                info,
                regions,
                binder_init_sites,
            ) {
                super::super::escape::Route::Binder(r) => r.into_iter().collect(),
                super::super::escape::Route::Prologue(rs) => rs,
                super::super::escape::Route::Ambiguous => regions.clone(),
                super::super::escape::Route::Unrouted => Vec::new(),
            };
            for &r in regions {
                *holder_count.entry(r).or_default() += 1;
                if let Some(us) = b_uses {
                    uses.entry(r).or_default().extend(us.iter().copied());
                }
                if let Some(&d) = def {
                    if route_regions.contains(&r) || info.cell_release_regions.contains(&r) {
                        anchors.entry(r).or_default().push(d);
                    }
                }
                if unsafe_holder && !info.cell_release_regions.contains(&r) {
                    tainted.insert(r);
                }
            }
        }
        // The allocation sites alone, for the loop-invariant guard. The full
        // anchor set unions holder DEF sites in for the live-in premise, and a
        // cell carried across a loop is defined outside it while the values it
        // stores are born inside — reading the cell's def as an allocation would
        // refuse the per-iteration release exactly where it is correct. A region
        // with no recorded allocation is allocated elsewhere (a parameter's
        // content), and stays refused by the guard's conservative reading of an
        // empty set.
        let mut allocs: HashMap<Region, Vec<HirId>> = HashMap::new();
        for (&alloc_id, &r) in &info.alloc_region {
            anchors.entry(r).or_default().push(alloc_id);
            allocs.entry(r).or_default().push(alloc_id);
        }
        let mut reads: HashMap<Region, Vec<HirId>> = HashMap::new();
        for (&read_id, containers) in &info.uncounted_read_sites {
            for &r in containers {
                reads.entry(r).or_default().push(read_id);
            }
        }
        Premises {
            uses,
            anchors,
            allocs,
            holder_count,
            reads,
            tainted,
        }
    }

    /// A region whose release is owned by another mechanism, so compensating it
    /// would double-free: merge children (the root's single decref frees them),
    /// co-owned-group members, mutated-slot 1-slot containers, the
    /// already-suppressed, and the slot-route taints above.
    pub(super) fn excluded(&self, info: &RegionInfo, r: Region) -> bool {
        info.suppressed_decref_regions.contains(&r)
            || info.owned_group_members.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || info.merged_root(r) != r
            || self.tainted.contains(&r)
    }

    /// The region is named by exactly one holder binding, so the `tail` value
    /// route's slot is unambiguous.
    pub(super) fn single_holder(&self, r: Region) -> bool {
        self.holder_count.get(&r).copied().unwrap_or(0) == 1
    }
}
