// audited: 2026-09-28
//! The `Funnel` store at a call site: the containment it builds, and the store,
//! byte-copy and container sites compensation reads.
//!
//! docs/impl/region/effects.md
//! docs/impl/region/compensate.md

use super::*;

impl RegionInference {
    /// Record what a `Funnel` native's store leaves for the ownership inference and
    /// for `region::infer::compensate`. The store itself is runtime-counted, so it
    /// records no may-store edge.
    pub(super) fn record_funnel_store(
        &mut self,
        site: HirId,
        func: &Hir,
        arg_regions: &[Vec<Region>],
    ) {
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
        // only `region::infer::ownership` (`containment_edges`), never the
        // lowerer, so it changes no release.
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
                            self.containment_edges.push((site, v, c));
                        }
                    }
                }
            }
            // A value-RETAINING store funnel (`%put`/`%array-push`/`%add`)
            // increfs the stored value at runtime whether or not arg0's
            // container type is statically recognized. Record the stored value —
            // the LAST arg (the value; the key, if any, sits between container
            // and value) — site-keyed for `region::infer::compensate`'s per-arm decref
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
                        self.funnel_store_sites.insert(site, stored);
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
                        self.funnel_bytecopy_value_sites.insert(site, stored);
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
            // `region::infer::compensate` places the balancing per-arm release. This
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
                    .insert(site, container_regions.to_vec());
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
                    .insert(site, container_regions.to_vec());
            }
        }
    }
}
