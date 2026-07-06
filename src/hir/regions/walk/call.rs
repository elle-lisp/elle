use super::*;

impl RegionInference {
    pub(super) fn walk_call(&mut self, hir: &Hir) -> Vec<Region> {
        let HirKind::Call { func, args, .. } = &hir.kind else {
            unreachable!("walk_call: non-Call HIR kind")
        };
        let _ = self.walk(func);
        let arg_regions: Vec<Vec<Region>> = args.iter().map(|a| self.walk(&a.expr)).collect();

        // Always register the Call node so the lowerer has a
        // region for the bytecode Call instruction.
        let call_r = self.alloc_here(hir.id);
        // Track that this region's runtime ID is whatever the
        // callee returns — the caller can't statically name it.
        // `call_r` is a marker, not a prediction: the lowerer
        // emits a value-based `DecrefValueRegion` at this
        // binding's decref_point, decreffing the *runtime* region of
        // the returned value. That reference was handed to us by
        // the callee's `IncrefValueRegion` (see `HirKind::Return`
        // / `src/hir/retain.rs`). No arg-embedding edge: the old
        // `arg → call_r` pin emitted an `IncrefRegion(arg)` that
        // never balanced (the phantom `call_r` never cascaded) —
        // the call-result leak. Arg lifetimes are now carried by
        // the callee retaining whatever it actually returns.
        self.call_result_regions.insert(call_r);

        // A mutable *retaining* container's fresh result (`@array`/
        // `@struct`): a later `Funnel` store into it recovers the
        // containment the funnel records only at runtime (no
        // `cross_region_refs` edge exists to build the subtree from). Keyed
        // on RetType, not effect — the container ctor is `Fresh`, but it is
        // the return type that says "mutable array/struct"; `@string`/
        // `@bytes` return non-container types and are excluded (they copy
        // bytes into the container, retaining no region).
        if matches!(
            self.call_rettype(func),
            Some(
                crate::primitives::def::RetType::MutableArray
                    | crate::primitives::def::RetType::MutableStruct
            )
        ) {
            self.mutable_container_regions.insert(call_r);
        }

        // Try inlining the callee's lambda body so intrinsics
        // inside the body produce the right edges at this
        // call site. Inlining only runs when the callee binds
        // a known immutable Lambda.
        if let Some(result) = self.try_inline_call(func, &arg_regions, hir.id) {
            return result;
        }

        // Opaque fallback, keyed on the callee's declared
        // RegionEffect (docs/impl/region-effects.md "Native region effects"):
        // - Immediate/Fresh/PassThrough store no argument — no
        //   may-store edges. An edge here becomes a compile-time
        //   IncrefRegion balanced only by the target's free-time
        //   cascade IF the store actually happens; for a
        //   never-storing native it never balances (the arg-clique
        //   leak class; region-native-effect-clique-leak.lisp).
        // - Stores{args}: a directed edge from each listed
        //   argument's regions to each OTHER heap argument's
        //   regions (the possible in-argument targets). Stores
        //   into the result or an external structure are
        //   runtime-counted (alloc-scan / store funnel /
        //   incref_for_escape) and need no edge.
        // - Mixed/Unknown/undeclared: each pair of heap args may
        //   store the other — the full mutual clique (over-keep,
        //   never mis-free).
        use crate::primitives::def::RegionEffect;
        match self.call_effect(func) {
            Some(RegionEffect::Fresh) => {
                // A Fresh result is freshly allocated in the call's own
                // region — genuinely caller-owned, so it is an Owned
                // candidate for the forest even though it is a call-result
                // placeholder for baseline release (region-effects.md
                // § Fresh; `RegionInfo::fresh_result_regions`). Recording
                // this is the only effect on the walk; release is unchanged
                // (value-gated `DecrefValueRegion`, as for any call-result),
                // and no may-store edge is added (a Fresh native stores no
                // argument outside the result).
                self.fresh_result_regions.insert(call_r);

                // Record `result ⊇ arg` containment for each argument the native
                // EMBEDS into its fresh result (`call_embeds` — the per-primitive
                // embed declaration; `with-traits` embeds arg 1, the trait table,
                // into the cloned result's `traits` side-field). This is the
                // compile-time analog of the runtime alloc-scan
                // (`find_object_cross_refs`, which enumerates the `traits`
                // side-field) that counts the same embedding at allocation:
                // without it the ownership forest cannot see a captured value flow
                // OUT through an escaping result and would fold it into the
                // capturing closure's Owned subtree, freeing it under the escaped
                // result's still-live reference. The edge feeds only
                // `regions::ownership` (`containment_edges`), never an
                // `IncrefRegion` (the alloc-scan counts the embedding at runtime)
                // — behavior-preserving, exactly like the funnel-recovered
                // containment below.
                for &i in self.call_embeds(func) {
                    if let Some(embedded_regions) = arg_regions.get(i) {
                        for &v in embedded_regions {
                            if v != call_r {
                                self.containment_edges.push((hir.id, v, call_r));
                            }
                        }
                    }
                }
            }
            Some(RegionEffect::Funnel) => {
                // The store is runtime-counted, so NO may-store edge — a
                // compile-time `IncrefRegion` would double-count against the
                // container's single free-time cascade decref. But the
                // ownership inference needs the *containment* the funnel
                // builds (for subtree membership), which is otherwise lost on
                // this path. Recover it structurally (no incref) when the
                // container argument — arg0, the funnel convention — is a
                // mutable retaining container: `container ⊇ each other heap
                // arg`. A `@string`/`@bytes` container is absent from
                // `mutable_container_regions` (non-container RetType), so its
                // byte-copying store correctly records nothing. The edge feeds
                // only `regions::ownership` (`containment_edges`), never the
                // lowerer — behavior-preserving.
                if let Some(container_regions) = arg_regions.first() {
                    let containers: Vec<Region> = container_regions
                        .iter()
                        .copied()
                        .filter(|c| self.mutable_container_regions.contains(c))
                        .collect();
                    for vs in arg_regions.iter().skip(1) {
                        for &v in vs {
                            for &c in &containers {
                                if v != c {
                                    self.containment_edges.push((hir.id, v, c));
                                }
                            }
                        }
                    }
                    // A value-RETAINING store funnel (`%put`/`%array-push`/`%add`,
                    // not `%del`/`%string-push`) increfs the stored value at runtime
                    // whether or not arg0's container type is statically recognized.
                    // Record the stored value — the LAST arg (the value; the key, if
                    // any, sits between container and value) — site-keyed for
                    // `regions::compensate`'s per-arm decref safety gate, even when no
                    // `containment_edge` is built (a parameter container, the
                    // `put`/`set` dispatch case). A non-retaining `Funnel` (`%del`
                    // removes/decrefs; `%string-push` byte-copies) records nothing:
                    // a per-arm decref there would double-free or over-free.
                    if self.is_retaining_store(func) {
                        if let Some(value_regions) = arg_regions.last() {
                            let stored: Vec<Region> = value_regions
                                .iter()
                                .copied()
                                .filter(|&v| !container_regions.contains(&v))
                                .collect();
                            if !stored.is_empty() {
                                self.funnel_store_sites.insert(hir.id, stored);
                            }
                        }
                    }
                }
            }
            Some(RegionEffect::Immediate | RegionEffect::PassThrough | RegionEffect::Opaque) => {
                // `Opaque` stores no argument (every arg is copied out —
                // to a Rust String/Vec, the kernel, a fresh structure),
                // so it carries NO arg clique, exactly like Immediate/
                // PassThrough. Its result is non-fresh (on the scheduler
                // heap, not the call's own region), so it is NOT recorded
                // in `fresh_result_regions` — it still contributes the
                // value-released call-result region below
                // (`call_returns_immediate` is false for Opaque). This is
                // the variant that keeps the clique keyed on the *store*,
                // not the result shape (docs/impl/region-effects.md § Opaque).
            }
            Some(RegionEffect::Stores { args: stored }) => {
                // A declared native's uncounted store is real: its edges are
                // HARD — the lowerer emits the incref for a call-result source
                // by value. Containment (into another arg or an external
                // structure), NOT a frontier crossing — no Shared seed.
                self.record_store_edges(hir.id, stored, &arg_regions);
            }
            Some(RegionEffect::Sends { args: stored }) => {
                // A fiber-crossing send (`chan/send`): the same edge/lifetime
                // accounting as `Stores` (keep the message alive in the channel
                // buffer until the receiving fiber takes it). The fiber-frontier
                // escape of the message is escape's judgment (`analyze_escape`'s
                // fiber/send facet, projected by `regions::escape`), not a
                // solver-recorded source set.
                self.record_store_edges(hir.id, stored, &arg_regions);
            }
            Some(RegionEffect::Mixed | RegionEffect::Unknown) => {
                // A registered NATIVE whose store behaviour is
                // uncounted (Mixed) or unexamined (Unknown) can reach
                // value/ internals and store an argument *uncounted* —
                // invisible to the funnel seam and the solver — so the
                // full mutual clique is its only cover. Its edges are
                // HARD (the lowerer increfs a call-result source by
                // value; docs/impl/region-effects.md "Hard edges").
                self.hard_edge_sites.insert(hir.id);
                let heap_args: Vec<Region> = arg_regions.iter().flatten().copied().collect();
                for i in 0..heap_args.len() {
                    for j in (i + 1)..heap_args.len() {
                        self.record_edge(hir.id, heap_args[i], heap_args[j]);
                        self.record_edge(hir.id, heap_args[j], heap_args[i]);
                    }
                }
            }
            None => {
                // An opaque USER-FN call (callee is not a registered
                // primitive — `call_effect` returns None for a user
                // binding, a shadowed name, or a non-Var callee).
                // Emit NO clique edges: a user fn is ordinary Elle
                // code, and Elle code can store into a mutable
                // container only through the runtime-counted
                // mutable-store funnel (Rule 5, statically complete),
                // or via a counted edge in the callee's OWN
                // compilation. So every store on an argument is
                // already counted at its site, and a caller-side
                // clique incref is pure redundancy that leaks one
                // region per alloc-region heap argument per call
                // (pinned by region-userfn-clique-noleak.lisp). Unlike a
                // Mixed/Unknown native, a user fn cannot perform an
                // UNCOUNTED store, so there is nothing for the clique
                // to cover. (Call-result sources were already a
                // slot-based no-op here; this drops the alloc-region
                // leak that remained — docs/impl/region-effects.md
                // "What the solver derives", the user-functions case.)
            }
        }

        if self.call_returns_immediate(func) {
            Vec::new()
        } else {
            vec![call_r]
        }
    }
}
