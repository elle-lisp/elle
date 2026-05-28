//! A-normal form (ANF) lift.
//!
//! Names every allocating expression by wrapping it in a synthetic
//! `let` whose body is the bound variable. After this pass, every
//! heap-allocating value has a `Binding` — meaning the lowerer can
//! key slot ownership entirely off `binding_to_slot`, with no shadow
//! mechanism for un-named call results.
//!
//! Example rewrite:
//!
//! ```text
//! (g (f x))    =>    (g (let [t0 (f x)] t0))
//! ```
//!
//! `t0` is a synthetic immutable binding. Region inference (which runs
//! after ANF) sees `f`'s call result as bound to `t0`, so escape
//! analysis owns its lifetime through a single mechanism.
//!
//! Pipeline placement: immediately after `functionalize`, before
//! `typeinfer` and region analysis.
//!
//! Status: pass is wired into every pipeline callsite but currently a
//! no-op. The actual rewrite, `Hir::allocates`, `pattern_allocates`,
//! and the matching lowerer changes (`region_to_slot`, removing
//! `wrap_call_with_release_slot`) land in a follow-up commit. The
//! design lives in `anf-plan.md`.

use super::arena::BindingArena;
use super::expr::Hir;

/// Run the ANF lift on a HIR tree.
///
/// Currently a no-op: gives the pipeline a stable callsite for the
/// real transform to land into without changing observed behavior.
pub fn anf_lift(_hir: &mut Hir, _arena: &mut BindingArena) {}
