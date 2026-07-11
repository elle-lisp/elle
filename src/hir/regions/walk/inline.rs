//! Callee inlining for the region walk: temporarily bind a Var-callee Lambda's
//! params to the caller's arg regions and re-walk its body so intrinsics buried
//! inside the callee emit their cross-region edges at the call site.

use super::*;

impl RegionInference {
    /// Try to inline a Call's callee Lambda body for region analysis.
    ///
    /// When the callee is a Var whose binding has a known Lambda init
    /// (recorded in `binding_lambda`), temporarily bind the Lambda's
    /// params to the caller's arg source regions and walk the body.
    /// This lets the walk see intrinsics inside the body (e.g.
    /// `%array-push` inside `push`) and emit the corresponding
    /// cross-region edges at the call site.
    ///
    /// Returns `Some(result_regions)` when inlining succeeded;
    /// `None` to fall back to opaque-call handling.
    pub(super) fn try_inline_call(
        &mut self,
        func: &Hir,
        arg_regions: &[Vec<Region>],
        _call_id: HirId,
    ) -> Option<Vec<Region>> {
        // Only inline Var callees.
        let binding = match &func.kind {
            HirKind::Var(b) => *b,
            _ => return None,
        };
        // Must be immutable and have a known Lambda body.
        let bi = self.arena().get(binding);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        let lambda_ptr = *self.binding_lambda.get(&binding)?;
        // Guard against infinite recursion (max 4 levels).
        if self.inline_depth >= 4 {
            return None;
        }
        // SAFETY: lambda_ptr points into the HIR tree which outlives
        // the RegionInference (both live for the analyze_regions call).
        let lambda = unsafe { &*lambda_ptr };
        let (params, rest_param, body) = match &lambda.kind {
            HirKind::Lambda {
                params,
                rest_param,
                body,
                ..
            } => (params, rest_param, body),
            _ => return None,
        };
        // Save and bind params to caller's arg regions.
        let mut saved: Vec<(Binding, Option<Vec<Region>>)> = Vec::new();
        for (i, p) in params.iter().enumerate() {
            saved.push((*p, self.binding_regions.get(p).cloned()));
            let regions = arg_regions.get(i).cloned().unwrap_or_default();
            self.binding_regions.insert(*p, regions);
            self.binding_region.insert(*p, self.current_region);
        }
        if let Some(rp) = rest_param {
            saved.push((*rp, self.binding_regions.get(rp).cloned()));
            self.binding_regions.insert(*rp, Vec::new());
            self.binding_region.insert(*rp, self.current_region);
        }
        // Mark the caller's arg regions live for this inline so a `Return`
        // reached in the body does not extend a caller region's `decref_point`
        // to a callee node (see `inline_bound_regions`). Track only those we
        // newly add — a region an OUTER inline already marked must stay marked
        // when this one exits.
        let mut newly_bound: Vec<Region> = Vec::new();
        for regions in arg_regions {
            for &r in regions {
                if self.inline_bound_regions.insert(r) {
                    newly_bound.push(r);
                }
            }
        }
        self.inline_depth += 1;
        let result = self.walk(body);
        self.inline_depth -= 1;
        for r in newly_bound {
            self.inline_bound_regions.remove(&r);
        }
        // Restore saved param region sets.
        for (p, prev) in saved {
            match prev {
                Some(v) => {
                    self.binding_regions.insert(p, v);
                }
                None => {
                    self.binding_regions.remove(&p);
                }
            }
        }
        Some(result)
    }
}
