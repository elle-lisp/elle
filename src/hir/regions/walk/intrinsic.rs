use super::*;

impl RegionInference {
    pub(super) fn walk_intrinsic(&mut self, hir: &Hir) -> Vec<Region> {
        let HirKind::Intrinsic { op, args } = &hir.kind else {
            unreachable!("walk_intrinsic: non-Intrinsic HIR kind")
        };
        let arg_regions: Vec<Vec<Region>> = args.iter().map(|a| self.walk(a)).collect();

        // %array-push(coll, val): val flows into coll. The monomorphic
        // %push-array / %push-array-mut share this type-blind edge for now;
        // the precise Fresh-vs-funnel split (funnel_store_edges) is a later
        // slice.
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
        // %put(obj, key, val): val flows into obj. Monomorphic %put-struct /
        // %put-array (+ -mut) share this type-blind edge for now; the precise
        // split is the funnel_store_edges slice.
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
        // %get is region-transparent: it borrows an existing value
        // out of arg 0's region — no allocation, no new region. The
        // result lives in arg 0's region(s).
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
            // %pair: car and cdr are stored inside the Pair.
            // Edge from each arg's regions to the pair's region.
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
