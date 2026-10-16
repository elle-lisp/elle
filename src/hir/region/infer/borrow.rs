// audited: 2026-10-16
//! The borrow pass: the containers each name's value is read out of with no count, carried to every name the value reaches.
//!
//! docs/impl/region/borrow.md
//!
//! A read's container is the walk's record (`uncounted_read_sites`). This pass
//! carries it on through the forms that hand a value on unchanged, to a fixed
//! point, because a `loop` parameter takes the borrow of each `recur` that
//! reaches it. Nothing here counts a holder: the names it fills hold no
//! reference.

use super::*;
use crate::hir::expr::{BlockId, IntrinsicOp};

/// What the pass found: where each borrow reaches, and the consuming points it
/// must still be live at.
#[derive(Default)]
pub(super) struct Borrows {
    /// Binding → the container regions its value is borrowed out of.
    pub(super) bindings: HashMap<Binding, Vec<Region>>,
    /// Read site → every container region the read borrows out of: its own
    /// operand's regions and the borrow that operand carries.
    pub(super) reads: HashMap<HirId, Vec<Region>>,
    /// `Destructure` node → the borrow its value carries. The extraction reads
    /// the value after the value's own last read.
    pub(super) destructures: Vec<(HirId, Vec<Region>)>,
    /// `Block` node → the borrow its `break`s carry. The block hands it to
    /// whatever consumes the block's value.
    pub(super) blocks: Vec<(HirId, Vec<Region>)>,
}

/// Carry every borrow in `hir` to the names it reaches.
///
/// `info` must already hold the walk's read sites and the reassign gate's
/// overwrite sites, which say which `assign` takes a count of its own.
pub(super) fn compute_borrows(hir: &Hir, info: &RegionInfo, arena: &BindingArena) -> Borrows {
    let mut pass = BorrowPass {
        info,
        arena,
        bindings: HashMap::new(),
        grew: false,
        loops: Vec::new(),
        breaks: HashMap::new(),
        reads: HashMap::new(),
        destructures: Vec::new(),
        blocks: Vec::new(),
    };
    loop {
        pass.grew = false;
        pass.reads.clear();
        pass.destructures.clear();
        pass.blocks.clear();
        pass.visit(hir);
        if !pass.grew {
            break;
        }
    }
    Borrows {
        bindings: pass.bindings,
        reads: pass.reads,
        destructures: pass.destructures,
        blocks: pass.blocks,
    }
}

struct BorrowPass<'a> {
    info: &'a RegionInfo,
    arena: &'a BindingArena,
    bindings: HashMap<Binding, Vec<Region>>,
    /// Whether this sweep added a region to any binding's borrow.
    grew: bool,
    /// The parameters of each enclosing `Loop`, innermost last. A `recur`
    /// hands its arguments to the innermost.
    loops: Vec<Vec<Binding>>,
    /// The borrow each open block's `break`s carry.
    breaks: HashMap<BlockId, Vec<Region>>,
    reads: HashMap<HirId, Vec<Region>>,
    destructures: Vec<(HirId, Vec<Region>)>,
    blocks: Vec<(HirId, Vec<Region>)>,
}

/// Add each region of `from` that `into` lacks, keeping `into` sorted.
fn join(into: &mut Vec<Region>, from: &[Region]) -> bool {
    let mut grew = false;
    for &r in from {
        if let Err(at) = into.binary_search_by_key(&r.0, |x| x.0) {
            into.insert(at, r);
            grew = true;
        }
    }
    grew
}

impl BorrowPass<'_> {
    fn lend(&mut self, b: Binding, borrow: &[Region]) {
        if borrow.is_empty() {
            return;
        }
        if join(self.bindings.entry(b).or_default(), borrow) {
            self.grew = true;
        }
    }

    /// Whether the store `assign` makes takes a count of its own: a captured
    /// cell's store, or a 1-slot container's counted overwrite
    /// (`Lowerer::lower_assign`). Such a store protects the value from the
    /// store on, so the borrow stops there.
    fn assign_counts(&self, site: HirId, target: Binding) -> bool {
        self.arena.get(target).needs_capture()
            || (self.info.drop_on_overwrite_sites.contains(&site)
                && !self.info.donated_overwrite_sites.contains(&site))
    }

    /// The borrow `h`'s value carries, recording what each node passes on.
    fn visit(&mut self, h: &Hir) -> Vec<Region> {
        match &h.kind {
            HirKind::Var(b) => self.bindings.get(b).cloned().unwrap_or_default(),

            HirKind::Intrinsic { op, args } => {
                let operand = args.first().map(|a| self.visit(a)).unwrap_or_default();
                for a in args.iter().skip(1) {
                    self.visit(a);
                }
                if !matches!(
                    op,
                    IntrinsicOp::First | IntrinsicOp::Rest | IntrinsicOp::Get
                ) {
                    return Vec::new();
                }
                let mut borrow = Vec::new();
                if let Some(direct) = self.info.uncounted_read_sites.get(&h.id) {
                    join(&mut borrow, direct);
                }
                join(&mut borrow, &operand);
                if !borrow.is_empty() {
                    self.reads.insert(h.id, borrow.clone());
                }
                borrow
            }

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    let borrow = self.visit(init);
                    self.lend(*b, &borrow);
                }
                self.visit(body)
            }
            HirKind::Define { binding, value } => {
                let borrow = self.visit(value);
                self.lend(*binding, &borrow);
                borrow
            }
            HirKind::Assign { target, value } => {
                let borrow = self.visit(value);
                if !self.assign_counts(h.id, *target) {
                    self.lend(*target, &borrow);
                }
                borrow
            }
            HirKind::SetCell { cell, value } => {
                // A cell store increfs what it stores, so the borrow stops at
                // the cell.
                self.visit(cell);
                self.visit(value)
            }
            HirKind::Destructure { pattern, value, .. } => {
                let borrow = self.visit(value);
                for b in pattern.bindings().bindings {
                    self.lend(b, &borrow);
                }
                if !borrow.is_empty() {
                    self.destructures.push((h.id, borrow.clone()));
                }
                borrow
            }

            HirKind::Begin(exprs) => {
                let mut last = Vec::new();
                for e in exprs {
                    last = self.visit(e);
                }
                last
            }
            HirKind::Block { block_id, body, .. } => {
                let mut last = Vec::new();
                for e in body {
                    last = self.visit(e);
                }
                if let Some(broken) = self.breaks.remove(block_id) {
                    self.blocks.push((h.id, broken.clone()));
                    join(&mut last, &broken);
                }
                last
            }
            HirKind::Break { block_id, value } => {
                let borrow = self.visit(value);
                if !borrow.is_empty() {
                    join(self.breaks.entry(*block_id).or_default(), &borrow);
                }
                Vec::new()
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.visit(cond);
                let mut out = self.visit(then_branch);
                let other = self.visit(else_branch);
                join(&mut out, &other);
                out
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut out = Vec::new();
                for (test, body) in clauses {
                    self.visit(test);
                    let arm = self.visit(body);
                    join(&mut out, &arm);
                }
                if let Some(e) = else_branch {
                    let arm = self.visit(e);
                    join(&mut out, &arm);
                }
                out
            }
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                let mut out = Vec::new();
                for e in exprs {
                    let arm = self.visit(e);
                    join(&mut out, &arm);
                }
                out
            }
            HirKind::Match { value, arms } => {
                let scrutinee = self.visit(value);
                let mut out = Vec::new();
                for (pattern, guard, body) in arms {
                    for b in pattern.bindings().bindings {
                        self.lend(b, &scrutinee);
                    }
                    if let Some(g) = guard {
                        self.visit(g);
                    }
                    let arm = self.visit(body);
                    join(&mut out, &arm);
                }
                out
            }

            HirKind::Loop { bindings, body } => {
                for (b, init) in bindings {
                    let borrow = self.visit(init);
                    self.lend(*b, &borrow);
                }
                self.loops.push(bindings.iter().map(|(b, _)| *b).collect());
                let out = self.visit(body);
                self.loops.pop();
                out
            }
            HirKind::Recur { args } => {
                let params = self.loops.last().cloned().unwrap_or_default();
                for (i, a) in args.iter().enumerate() {
                    let borrow = self.visit(a);
                    if let Some(&p) = params.get(i) {
                        self.lend(p, &borrow);
                    }
                }
                Vec::new()
            }

            HirKind::MakeCell { value } | HirKind::Return { value } => self.visit(value),
            HirKind::DerefCell { cell } => self.visit(cell),
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    self.visit(k);
                    self.visit(v);
                }
                self.visit(body)
            }

            // A lambda's body runs in its own activation, where no enclosing
            // `recur` reaches.
            HirKind::Lambda(lambda) => {
                let saved = std::mem::take(&mut self.loops);
                self.visit(&lambda.body);
                self.loops = saved;
                Vec::new()
            }

            // A call, an emit and an eval each hand back a value carrying a
            // reference of its own, so the borrow ends at them.
            HirKind::Call { .. } | HirKind::Emit { .. } | HirKind::Eval { .. } => {
                h.for_each_child(|c| {
                    self.visit(c);
                });
                Vec::new()
            }
            HirKind::While { cond, body } => {
                self.visit(cond);
                self.visit(body);
                Vec::new()
            }

            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_)
            | HirKind::Error => Vec::new(),
        }
    }
}
