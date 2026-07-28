use super::*;

impl RegionInference {
    pub(super) fn walk_call(&mut self, hir: &Hir) -> Vec<Region> {
        let HirKind::Call {
            func,
            args,
            is_tail,
            ..
        } = &hir.kind
        else {
            unreachable!("walk_call: non-Call HIR kind")
        };
        // A tail call to a bytecode CLOSURE replaces this frame, so control never
        // arrives at the enclosing branch's merge label; a tail call to a native
        // pushes no frame and falls through to it. The branch-arm release window
        // needs that distinction to know whether its anchor is a point every arm
        // reaches (docs/impl/region/mechanism.md § "A release inside one arm is
        // not a release on the other arms"). Recorded before the inline attempt
        // below, which returns early for a known lambda callee — itself a
        // frame-replacing one.
        if *is_tail && !self.is_native_callee(func) {
            self.frame_replacing_tail_calls.insert(hir.id);
        }
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

        // A declared-`RetType::Fiber` result (`fiber/new`): the region holds a
        // fiber, which acquires aliases by merely running (the scheduler's
        // parent/child chain, the `fiber/child`/`fiber/parent` graph reads), so
        // it is never a member of any region-rooted ownership cut —
        // `ownership::inputs::not_ownable` refuses this class and the region
        // reclaims on the RC baseline (docs/impl/region/adopt.md § "The fiber
        // member — refused at the class level"). Keyed on RetType, not effect,
        // so the class holds however the mint is declared.
        if matches!(
            self.call_rettype(func),
            Some(crate::primitives::def::RetType::Fiber)
        ) {
            self.fiber_result_regions.insert(call_r);
        }

        // Try inlining the callee's lambda body so intrinsics
        // inside the body produce the right edges at this
        // call site. Inlining only runs when the callee binds
        // a known immutable Lambda.
        if let Some(result) = self.try_inline_call(func, &arg_regions, hir.id) {
            return result;
        }

        // Opaque fallback, keyed on the callee's declared
        // RegionEffect (docs/impl/region/effects.md "Native region effects"):
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
                // placeholder for baseline release (region/effects.md
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
                    // A value-RETAINING store funnel (`%put`/`%array-push`/`%add`)
                    // increfs the stored value at runtime whether or not arg0's
                    // container type is statically recognized. Record the stored value —
                    // the LAST arg (the value; the key, if any, sits between container
                    // and value) — site-keyed for `regions::compensate`'s per-arm decref
                    // safety gate, even when no `containment_edge` is built (a parameter
                    // container, the `put`/`set` dispatch case). A per-arm decref there
                    // releases only the wrapper's stranded owned reference; the
                    // container's retain keeps the value's RC ≥ 1.
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
                    // A BYTE-COPY store funnel (`%string-push`/`%string-push-mut`/
                    // `%bytes-push`) COPIES the value's bytes into the container and
                    // touches NEITHER its incref NOR its decref. So a dispatch wrapper's
                    // `val` param — used across arms, freed in one — strands on the
                    // sibling arms exactly as a retaining store's does, and the per-arm
                    // release is `val`'s TRUE last use (not a redundant strand, and not
                    // the `%del` double-free: `%del` decrefs in-body and is excluded).
                    // Recorded separately (`funnel_bytecopy_value_sites`) so the
                    // compensation's guard documents the distinct invariant.
                    if self.is_bytecopy_store(func) {
                        if let Some(value_regions) = arg_regions.last() {
                            let stored: Vec<Region> = value_regions
                                .iter()
                                .copied()
                                .filter(|&v| !container_regions.contains(&v))
                                .collect();
                            if !stored.is_empty() {
                                self.funnel_bytecopy_value_sites.insert(hir.id, stored);
                            }
                        }
                    }
                    // A MONOMORPHIC store/remove funnel (`%put-*`/`%add-set*`/
                    // `%push-array*`/`%del-*`, either mutability) is the target of a
                    // dispatch wrapper's `(match (type-of coll) …)` arm, and `coll` is
                    // used in EVERY arm (the scrutinee + each arm's funnel call) while
                    // its single `decref_point` sits in ONE arm — so the owned-param
                    // reference the wrapper holds to the container leaks on every OTHER
                    // arm's path. Record the container (arg0) site-keyed so
                    // `regions::compensate` places the balancing per-arm release. This
                    // is sound for both container flavours:
                    //   - a `-mut` funnel RETURNS the container pass-through, so the
                    //     container is return-escaping; the funnel's `pass_through_retain`
                    //     leaves the returned value's RC ≥ 1, so releasing the owned-param
                    //     reference can never drop the live result to zero (the
                    //     return-frontier exclusion is lifted for it in `compensate`);
                    //   - an IMMUTABLE funnel returns a FRESH copy, so the container is
                    //     genuinely dead in the arm — the ordinary owned-param release
                    //     the branch structure otherwise strands.
                    // Keyed on a recognized monomorphic container RetType (NOT the
                    // polymorphic `FirstArg`, whose container mutability is unproven).
                    use crate::primitives::def::RetType;
                    let rettype = self.call_rettype(func);
                    if matches!(
                        rettype,
                        Some(
                            RetType::Struct
                                | RetType::MutableStruct
                                | RetType::Array
                                | RetType::MutableArray
                                | RetType::Set
                                | RetType::MutableSet
                                | RetType::MutableString
                        )
                    ) {
                        self.funnel_container_sites
                            .insert(hir.id, container_regions.to_vec());
                    }
                    // The `-mut` PASS-THROUGH subset: the funnel returns the container
                    // (arg0) ITSELF, so the container IS the result and the caller
                    // already owns a reference to it. Recorded separately so the
                    // lowerer's ReturnValue suppression fires ONLY here (via
                    // `container_release_sites`, gated on this in `compensate`): an
                    // IMMUTABLE funnel returns a FRESH copy whose ReturnValue retain is
                    // the caller's move/reassign reference — suppressing it over-frees a
                    // result stored into a reassigned slot (the container is still
                    // compensated by `funnel_container_sites`, so its owned-param leak
                    // still closes; only the redundant-retain drop is withheld).
                    if matches!(
                        rettype,
                        Some(
                            RetType::MutableStruct
                                | RetType::MutableArray
                                | RetType::MutableSet
                                | RetType::MutableString
                        )
                    ) {
                        self.funnel_passthrough_sites
                            .insert(hir.id, container_regions.to_vec());
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
                // not the result shape (docs/impl/region/effects.md § Opaque).
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
                // value; docs/impl/region/effects.md "Hard edges").
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
                // leak that remained — docs/impl/region/effects.md
                // "What the solver derives", the user-functions case.)
            }
        }

        // The result side's analogue of the may-store clique above, split by how much the
        // callee's declaration pins down WHERE its result lives. Both feed the ownership
        // cut's alias obligation and the lowerer's release ORDER at a shared point, never
        // an `IncrefRegion` — so the baseline RC stream is unchanged. A
        // container-READ borrow is excluded from both: its result is an ELEMENT of arg0,
        // which the read edge below records with the tighter container and the same
        // bound — the `Funnel` reading here would otherwise be plainly wrong for it
        // (`get`/`first`/`rest` declare `Funnel`, and their result is emphatically not
        // the container).
        if !self.call_returns_immediate(func)
            && self.result_may_alias_args(func)
            && !self.is_container_read_borrow(func)
        {
            if matches!(self.call_effect(func), Some(RegionEffect::Funnel)) {
                // A `Funnel` says the result is arg0 in place or a fresh copy of it
                // (region/effects.md § `Funnel`) — the CONTAINER either way, never an
                // element interior to it. So the result is not a new region for the
                // lifetime obligation to bound: on the in-place path it resolves to arg0
                // and holds arg0's own counted pass-through reference, and where arg0 is
                // itself an adopted member its decref lands on the frozen region and
                // no-ops (region/adopt.md § "The lifetime obligation the root carries",
                // the emit-order paragraph). What it still carries is REACHABILITY — a
                // read out of the funnel's result is a read out of arg0 — so record the
                // identity with the container alone.
                if let Some(container_regions) = arg_regions.first() {
                    for &v in container_regions {
                        if v != call_r {
                            self.funnel_result_containers.push((hir.id, call_r, v));
                        }
                    }
                }
            } else {
                // Every other alias-capable callee is under no such claim: it may hand
                // back an argument itself (`concat` extends a mutable first argument in
                // place and returns it) or a value it read OUT of one (`last`), so
                // `call_r` — a placeholder relating to no member statically — can name a
                // frozen member. Adoption leaves the result's pass-through retain inert,
                // exactly as for a container read, so the root's drop must bound this
                // release too (`region_call_result_alias_uaf`). Reached only on the
                // opaque path: an INLINED callee returned above with the regions its body
                // really yields, which need no alias edge at all.
                for vs in &arg_regions {
                    for &v in vs {
                        if v != call_r {
                            self.opaque_result_aliases.push((hir.id, call_r, v));
                        }
                    }
                }
            }
        }

        // A native container element READ that BORROWS (`get`/`first`/`rest`): the value
        // handed back still lives inside the container passed as arg0 — the funnel
        // convention — and `call_r` (minted above) is the caller-side placeholder for it.
        // The dispatch takes the Rule 5 pass-through retain, so under RC the reader holds
        // its own counted reference and the container needs no lifetime extension; what
        // that retain cannot cover is ADOPTION, which freezes the member's RC and leaves
        // it inert. Record `(alias, container)` so the ownership cut refuses to claim a
        // member this alias may still name, and so the lowerer orders the alias's
        // page-reading release ahead of the container's where they share a point
        // (region/adopt.md § "The lifetime obligation the root carries";
        // `region_container_read_borrow_uaf`). A moves-out REMOVE extracts its element
        // rather than borrowing it and is excluded (`is_container_read_borrow`).
        if self.is_container_read_borrow(func) {
            if let Some(container_regions) = arg_regions.first() {
                for &container in container_regions {
                    if container != call_r {
                        self.counted_read_aliases.push((hir.id, call_r, container));
                    }
                }
            }
        }

        // A moves-out ∩ PassThrough native (`%pop`/`%pop-array*`) removes a
        // pre-existing heap element from a container and escape-retains it IN-BODY
        // (`arena::pop_with_decref` increfs before releasing the container), and
        // `dispatch_native_call` skips its own pass-through retain (`def.moves_out`).
        // So in TAIL position the lowerer's extra ReturnValue `IncrefValueRegion`
        // double-counts against that in-body retain and frees the element under a
        // live reference (`region_pop_tail_moves_out_uaf`). Record the site so the
        // lowerer drops that redundant retain — the moves-out analogue of
        // `container_release_sites`. Gated to `PassThrough` (`call_moves_out_passthrough`)
        // so a moves-out native with a FRESH result (`@string` grapheme / `@bytes`
        // int pop, `Funnel`/`Immediate`) is EXCLUDED: its result is born rc=1 with no
        // in-body retain and NEEDS the tail retain to survive the caller's read.
        if self.call_moves_out_passthrough(func) {
            self.moves_out_release_sites.insert(hir.id);
        }

        // A moves-out REMOVE funnel (`%pop`/`%pop-string`/`%pop-bytes`) is dispatched
        // from a `pop` wrapper's `(match (type-of coll) …)` arm, and `coll` (the
        // container, arg0) is used in EVERY arm — scrutinee + each arm's funnel call —
        // while its single `decref_point` sits in ONE arm, so the owned-param reference
        // the wrapper holds strands on every OTHER arm's path (the F1b container strand
        // `add`/`del` have). Record arg0 as a container site so `regions::compensate`
        // places the balancing per-arm release. Recorded into `funnel_container_sites`
        // ONLY — NOT `funnel_passthrough_sites`: `pop` returns the ELEMENT, not the
        // container, so the container is genuinely DEAD in the arm (the immutable-funnel
        // treatment — a per-arm owned-param release, and NO tail-retain suppression on
        // the container's account; the element's own redundant tail retain is handled
        // separately by `moves_out_release_sites`). Keyed off the moves-out fact, not a
        // container RetType (pop's RetType is the element), so it covers the PassThrough
        // `%pop` arm and the fresh-result `%pop-string`/`%pop-bytes` arms alike.
        if self.call_moves_out(func) {
            if let Some(container_regions) = arg_regions.first() {
                if !container_regions.is_empty() {
                    self.funnel_container_sites
                        .insert(hir.id, container_regions.to_vec());
                }
            }
        }

        if self.call_returns_immediate(func) {
            Vec::new()
        } else {
            vec![call_r]
        }
    }
}
