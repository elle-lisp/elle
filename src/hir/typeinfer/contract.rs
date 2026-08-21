//! The intrinsic operand proofs: prove-or-reject, per docs/intrinsics.md.
//!
//! A `%`-intrinsic in call position is a compile-time type-checked request for
//! the fast lowering. Each op carries a soundness contract derived from what
//! its lowering actually trusts (the opcode handlers in `src/vm/` — wrong
//! operands there compute garbage, never a catchable error), and every
//! call-position use must discharge it from the inferred operand types:
//! proven ⇒ the site lowers and is silent; provably wrong **or unprovable** ⇒
//! compile error. Value-position uses are the registered `NativeFn`, which
//! validates at runtime — nothing to prove there.
//!
//! Call-position sites appear in two HIR shapes, checked identically: the
//! opcode `Intrinsic` node (non-storing ops) and the native funnel `Call`
//! whose callee is the `%`-named NativeFn (storing/copying ops — the
//! escape-correct region path).
//!
//! The div family (`%div`/`%rem`/`%mod`) carries a **value** obligation on top
//! of the type: the divisor must be provably nonzero (integer division by
//! zero has no silent total semantics). Nonzero facts flow like the type
//! narrowing does: a nonzero literal, a binding initialized from one, or a
//! diverging zero guard (`(when (%eq d 0) (error …))`) upstream of the site.
//! Reassignment invalidates the fact.
//!
//! The concern splits across three submodules: `table` (the declarative
//! contract row per op), `walk` (the evaluation-order traversal carrying
//! nonzero facts), and `check` (discharging one site's row).

use super::*;
use std::collections::HashSet;

mod check;
mod table;
mod walk;

// Sibling submodules reach these declarative/per-site helpers through the
// root's `super::*`; the entry point below reaches `walk`.
use check::check_op;
use table::{op_contract, Contract, ARRAY_FAMILY, STRUCT_FAMILY};
use walk::walk;

/// Nonzero-divisor flow environment: bindings proven ≠ 0 on the current path.
#[derive(Default, Clone)]
struct NonzeroEnv(HashSet<Binding>);

impl NonzeroEnv {
    fn apply(&mut self, facts: &[guard::Fact]) {
        for f in facts {
            if let guard::Fact::Nonzero(b) = f {
                self.0.insert(*b);
            }
        }
    }
    fn insert(&mut self, b: Binding) -> bool {
        self.0.insert(b)
    }
    fn invalidate(&mut self, b: Binding) {
        self.0.remove(&b);
    }
    fn proves(&self, b: Binding) -> bool {
        self.0.contains(&b)
    }
}

/// Check every call-position `%`-intrinsic in the tree against its contract.
/// Walks in evaluation order, carrying the nonzero-divisor facts.
pub(super) fn check_intrinsic_operand_proofs(
    hir: &Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Result<(), String> {
    let interner = TypeInterner::new();
    let mut env = NonzeroEnv::default();
    walk(hir, hir_types, arena, symbol_names, &interner, &mut env)
}
