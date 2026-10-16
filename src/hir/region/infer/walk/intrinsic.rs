// audited: 2026-10-16
//! The region walk's `%`-intrinsic arm: the edges a storing op records, the container a read borrows, the region an op mints.
//!
//! docs/impl/region/model.md
//! docs/impl/region/borrow.md

use super::*;

impl RegionInference {
    pub(super) fn walk_intrinsic(&mut self, hir: &Hir) -> Vec<Region> {
        let HirKind::Intrinsic { op, args } = &hir.kind else {
            unreachable!("walk_intrinsic: non-Intrinsic HIR kind")
        };
        let arg_regions: Vec<Vec<Region>> = args.iter().map(|a| self.walk(a)).collect();

        // %array-push(coll, val): val flows into coll. The monomorphic
        // %push-array / %push-array-mut share this type-blind edge.
        if matches!(
            *op,
            crate::hir::expr::IntrinsicOp::Push
                | crate::hir::expr::IntrinsicOp::PushArray
                | crate::hir::expr::IntrinsicOp::PushArrayMut
        ) {
            if let (Some(coll_rs), Some(val_rs)) = (arg_regions.first(), arg_regions.get(1)) {
                for &coll in coll_rs {
                    for &val in val_rs {
                        self.record_edge(hir.id, val, coll);
                    }
                }
            }
        }
        // %add-set(set, val): val flows into set — the set-family store edge,
        // exactly like %array-push's val→coll. The stored element is frozen by the
        // runtime body, but its region is what the container retains.
        if matches!(
            *op,
            crate::hir::expr::IntrinsicOp::AddSet | crate::hir::expr::IntrinsicOp::AddSetMut
        ) {
            if let (Some(set_rs), Some(val_rs)) = (arg_regions.first(), arg_regions.get(1)) {
                for &set in set_rs {
                    for &val in val_rs {
                        self.record_edge(hir.id, val, set);
                    }
                }
            }
        }
        // %put(obj, key, val): val flows into obj. Monomorphic %put-struct /
        // %put-array (+ -mut) share this type-blind edge.
        if matches!(
            *op,
            crate::hir::expr::IntrinsicOp::Put
                | crate::hir::expr::IntrinsicOp::PutStruct
                | crate::hir::expr::IntrinsicOp::PutArray
                | crate::hir::expr::IntrinsicOp::PutStructMut
                | crate::hir::expr::IntrinsicOp::PutArrayMut
        ) {
            if let (Some(coll_rs), Some(val_rs)) = (arg_regions.first(), arg_regions.get(2)) {
                for &coll in coll_rs {
                    for &val in val_rs {
                        self.record_edge(hir.id, val, coll);
                    }
                }
            }
        }

        use crate::hir::expr::IntrinsicOp;
        // The UNCOUNTED borrowing reads (`%get`/`%first`/`%rest`): the value handed back
        // still lives inside arg 0 — an element of the container, a pair's car/cdr — and
        // this inline opcode raises NO count on it (the native read's dispatch takes the
        // Rule 5 pass-through retain; an opcode has no dispatch). So arg 0's own lifetime
        // is what keeps the borrow alive, and arg 0 is in use for as long as the read's
        // RESULT is. Record its regions as the read's container. The borrow pass carries
        // them to every name the result reaches, and `analyze::decref` extends their
        // `decref_point` to the read's consumer and to those names' uses
        // (docs/impl/region/borrow.md).
        if matches!(
            op,
            IntrinsicOp::Get | IntrinsicOp::First | IntrinsicOp::Rest
        ) {
            if let Some(container_regions) = arg_regions.first() {
                if !container_regions.is_empty() {
                    self.uncounted_read_sites
                        .insert(hir.id, container_regions.to_vec());
                }
            }
        }
        // %get mints no region, and hands back arg 0's regions as well as the
        // borrow: the name it binds holds the container. A store of its result
        // then records an edge out of the container, which the ownership cut's
        // external-uniqueness test reads (docs/impl/region/borrow.md).
        // `%first` and `%rest` fall through to the non-allocating arm below.
        if matches!(op, IntrinsicOp::Get) {
            return arg_regions.into_iter().next().unwrap_or_default();
        }

        // %put / %del / %string-push / %array-push / %bytes-push are
        // conditionally-allocating natives: a *mutable* collection arg
        // is mutated in place (result = arg 0, a pass-through), an
        // *immutable* arg yields a fresh copy. The walk is type-blind,
        // so — exactly like a Call — give the result its OWN call-result
        // region: the handler mints a fresh region and
        // pass-through-retains (`run_alloc_intrinsic`), and the lowerer
        // emits a value-based `DecrefValueRegion` at the decref_point
        // that frees whatever *runtime* region the result actually
        // landed in (the minted region for a fresh copy, or arg 0's
        // region for the in-place case, balanced by the handler's
        // retain). This mirrors `dispatch_native_call` — call-position
        // uses of these storing ops lower as native funnel calls
        // (`IntrinsicOp::routes_native_funnel()`), and this arm keeps
        // the intrinsic-node shape's accounting aligned with that path.
        // The `val→coll` store edge recorded above carries the in-place
        // value retention.
        if matches!(
            op,
            IntrinsicOp::Put
                | IntrinsicOp::PutStruct
                | IntrinsicOp::PutArray
                | IntrinsicOp::PutStructMut
                | IntrinsicOp::PutArrayMut
                | IntrinsicOp::AddSet
                | IntrinsicOp::AddSetMut
                | IntrinsicOp::Del
                | IntrinsicOp::StringPush
                | IntrinsicOp::Push
                | IntrinsicOp::PushArray
                | IntrinsicOp::PushArrayMut
                | IntrinsicOp::BytesPush
        ) {
            let result_r = self.alloc_here(hir.id);
            self.call_result_regions.insert(result_r);
            return vec![result_r];
        }

        if op.allocates() {
            let result_r = self.alloc_here(hir.id);
            // %pair: car and cdr are stored inside the Pair. Edge from each
            // arg's regions to the pair's region. The allocation's scan counts
            // each such reference, so the edge is containment for the merge seed
            // and the ownership forest, and takes no static incref
            // (docs/impl/region/mechanism.md).
            if *op == IntrinsicOp::Pair {
                for ars in &arg_regions {
                    for &r in ars {
                        self.record_edge(hir.id, r, result_r);
                    }
                }
            }
            vec![result_r]
        } else {
            Vec::new()
        }
    }
}
