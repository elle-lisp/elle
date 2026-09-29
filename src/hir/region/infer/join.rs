// audited: 2026-09-29
//! The append seed: which allocations join the region of the container they are pushed into.
//!
//! docs/impl/region/colocation.md
//!
//! A value pushed into a container nothing ever takes a value out of can be born
//! in the container's region: its slot then dies with the container, and no other
//! member dies before it. The seed admits a container per function, from its uses,
//! and a push site per value, from its producer. Everything the seed cannot prove
//! it refuses, and a refused site mints fresh, which is always legal.

use super::*;
use crate::hir::expr::{CallArg, IntrinsicOp};
use crate::value::SymbolId;
use rustc_hash::FxHashSet;

/// The seed's answer for one compilation unit.
pub(super) struct Joins {
    /// Allocation site → the container binding whose region its value joins.
    pub(super) sites: HashMap<HirId, Binding>,
    /// Every region a join relates: the joined values' and their containers'.
    pub(super) regions: FxHashSet<Region>,
}

/// Run the append seed over every function in `hir`.
pub(super) fn compute_joins(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    call_class: &CallClassification,
) -> Joins {
    let seed = Seed {
        arena,
        info,
        call_class,
    };
    let mut joins = Joins {
        sites: HashMap::new(),
        regions: FxHashSet::default(),
    };
    seed.each_function(hir, &mut joins);
    joins
}

/// Where a use of a container sits, as far as the seed reads it.
#[derive(Clone, Copy)]
enum Pos<'h> {
    /// The value is discarded.
    Discarded,
    /// The value is the function's result.
    Returned,
    /// The value is an operand an inline intrinsic reads and keeps nothing of.
    Read,
    /// The value is argument `index` of the call `call`.
    Arg { call: &'h Hir, index: usize },
    /// Anywhere else: bound, stored, compared, branched on.
    Other,
}

/// One candidate container: an immutable binding of a fresh `@array`.
#[derive(Default)]
struct Container<'h> {
    /// The push sites, each the push call and the value it appends.
    pushes: Vec<(&'h Hir, &'h Hir)>,
    /// A use the bound refuses: the container is not append-only.
    refused: bool,
}

struct Seed<'a> {
    arena: &'a BindingArena,
    info: &'a RegionInfo,
    call_class: &'a CallClassification,
}

impl<'a> Seed<'a> {
    /// Seed each function body on its own: a container and its pushes live in one
    /// function, and a use inside a nested lambda is a capture.
    fn each_function(&self, h: &Hir, joins: &mut Joins) {
        if let HirKind::Lambda { body, .. } = &h.kind {
            self.seed_function(body, joins);
        }
        h.for_each_child(|c| self.each_function(c, joins));
    }

    fn seed_function(&self, body: &Hir, joins: &mut Joins) {
        let mut containers: HashMap<Binding, Container> = HashMap::new();
        self.find_containers(body, &mut containers);
        if containers.is_empty() {
            return;
        }
        self.visit(body, Pos::Discarded, false, &mut containers);
        for (container, uses) in containers {
            if uses.refused {
                continue;
            }
            for (push, value) in uses.pushes {
                self.admit(container, push, value, joins);
            }
        }
    }

    /// The bindings of this function that name a fresh `@array` and never change.
    fn find_containers<'h>(&self, h: &'h Hir, out: &mut HashMap<Binding, Container<'h>>) {
        match &h.kind {
            HirKind::Lambda { .. } => return,
            HirKind::Define { binding, value } => self.consider(*binding, value, out),
            HirKind::Let { bindings, .. } => {
                for (b, init) in bindings {
                    self.consider(*b, init, out);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| self.find_containers(c, out));
    }

    fn consider<'h>(&self, b: Binding, init: &Hir, out: &mut HashMap<Binding, Container<'h>>) {
        let info = self.arena.get(b);
        if info.is_mutated || !info.is_immutable || info.needs_capture() || info.is_file_scope {
            return;
        }
        let HirKind::Call { func, .. } = &init.kind else {
            return;
        };
        let Some(name) = self.primitive(func) else {
            return;
        };
        let fresh_array = self.call_class.ret_types.get(&name)
            == Some(&crate::primitives::def::RetType::MutableArray)
            && self.call_class.effects.get(&name)
                == Some(&crate::primitives::def::RegionEffect::Fresh);
        if fresh_array {
            out.entry(b).or_default();
        }
    }

    /// Walk a function body, classifying every use of a candidate container.
    fn visit<'h>(
        &self,
        h: &'h Hir,
        pos: Pos<'h>,
        nested: bool,
        containers: &mut HashMap<Binding, Container<'h>>,
    ) {
        match &h.kind {
            HirKind::Var(b) => {
                if let Some(uses) = containers.get_mut(b) {
                    if nested || !self.use_keeps_append_only(pos, uses) {
                        uses.refused = true;
                    }
                }
            }
            HirKind::Lambda { body, .. } => self.visit(body, Pos::Other, true, containers),
            HirKind::Return { value } => self.visit(value, Pos::Returned, nested, containers),
            HirKind::Call { func, args, .. } => {
                self.refuse_a_push_whose_result_flows_on(func, args, pos, containers);
                self.visit(func, Pos::Other, nested, containers);
                for (index, arg) in args.iter().enumerate() {
                    let arg_pos = if arg.spliced {
                        Pos::Other
                    } else {
                        Pos::Arg { call: h, index }
                    };
                    self.visit(&arg.expr, arg_pos, nested, containers);
                }
            }
            HirKind::Intrinsic { op, args } => {
                let arg_pos = if reads_only(*op) {
                    Pos::Read
                } else {
                    Pos::Other
                };
                for arg in args {
                    self.visit(arg, arg_pos, nested, containers);
                }
            }
            HirKind::Begin(exprs) => {
                let last = exprs.len().saturating_sub(1);
                for (i, e) in exprs.iter().enumerate() {
                    let p = if i == last { pos } else { Pos::Discarded };
                    self.visit(e, p, nested, containers);
                }
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.visit(cond, Pos::Other, nested, containers);
                self.visit(then_branch, pos, nested, containers);
                self.visit(else_branch, pos, nested, containers);
            }
            // ANF names a value it hands on unchanged: `(let [t e] t)` is `e`.
            HirKind::Let { bindings, body } if self.names_its_init(bindings, body) => {
                self.visit(&bindings[0].1, pos, nested, containers);
            }
            HirKind::Let { bindings, body } => {
                for (_, init) in bindings {
                    self.visit(init, Pos::Other, nested, containers);
                }
                self.visit(body, pos, nested, containers);
            }
            HirKind::Define { value, .. } => self.visit(value, Pos::Other, nested, containers),
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    self.visit(init, Pos::Other, nested, containers);
                }
                self.visit(body, pos, nested, containers);
            }
            _ => h.for_each_child(|c| self.visit(c, Pos::Other, nested, containers)),
        }
    }

    /// Refuse the container a push names when the push's own result flows on: an
    /// append returns its container in place, so that result is one more use of
    /// it. A push whose result is discarded or returned adds no other use.
    fn refuse_a_push_whose_result_flows_on<'h>(
        &self,
        func: &Hir,
        args: &[CallArg],
        pos: Pos<'h>,
        containers: &mut HashMap<Binding, Container<'h>>,
    ) {
        if matches!(pos, Pos::Discarded | Pos::Returned) {
            return;
        }
        let is_append = self
            .primitive(func)
            .is_some_and(|name| self.call_class.append_store_funnels.contains(&name));
        if !is_append {
            return;
        }
        let Some(first) = args.first() else {
            return;
        };
        if let HirKind::Var(b) = &first.expr.kind {
            if let Some(uses) = containers.get_mut(b) {
                uses.refused = true;
            }
        }
    }

    /// Whether a use of a container at `pos` keeps it append-only, recording the
    /// push site when it is one.
    fn use_keeps_append_only<'h>(&self, pos: Pos<'h>, uses: &mut Container<'h>) -> bool {
        match pos {
            Pos::Returned | Pos::Discarded | Pos::Read => true,
            Pos::Other => false,
            Pos::Arg { call, index } => {
                let HirKind::Call { func, args, .. } = &call.kind else {
                    return false;
                };
                let Some(name) = self.primitive(func) else {
                    return false;
                };
                if self.call_class.append_store_funnels.contains(&name) {
                    // Only as the container: the pushed value is the last argument,
                    // and a container pushed into another container is stored.
                    if index != 0 || args.len() != 2 {
                        return false;
                    }
                    uses.pushes.push((call, &args[1].expr));
                    return true;
                }
                self.stores_nothing(func, name, index)
            }
        }
    }

    /// Whether a native reads argument `index` and neither stores nor removes it.
    fn stores_nothing(&self, func: &Hir, name: SymbolId, index: usize) -> bool {
        use crate::primitives::def::RegionEffect;
        if self.call_class.container_read_funnels.contains(&name) {
            return !self.call_class.moves_out.contains(&name);
        }
        match self.primitive_effect(func) {
            Some(RegionEffect::Immediate | RegionEffect::Opaque) => true,
            Some(RegionEffect::Fresh) => !self
                .call_class
                .embeds
                .get(&name)
                .is_some_and(|e| e.contains(&index)),
            _ => false,
        }
    }

    /// Admit one push site: its value must be a fresh allocation held by nothing
    /// but the push.
    fn admit(&self, container: Binding, push: &Hir, value: &Hir, joins: &mut Joins) {
        let producer = self.unname(value);
        let Some(&region) = self.info.alloc_region.get(&producer.id) else {
            return;
        };
        let fresh = if self.info.call_result_regions.contains(&region) {
            self.info.fresh_result_regions.contains(&region)
        } else {
            matches!(
                producer.kind,
                HirKind::String(_)
                    | HirKind::QuoteConst(_)
                    | HirKind::Lambda { .. }
                    | HirKind::Intrinsic {
                        op: IntrinsicOp::Pair,
                        ..
                    }
            )
        };
        if !fresh || !self.held_only_by_temps(region) || self.stored_elsewhere(region, push.id) {
            return;
        }
        let Some(container_regions) = self.info.binding_source_regions.get(&container) else {
            return;
        };
        joins.sites.insert(producer.id, container);
        joins.regions.insert(region);
        joins.regions.extend(container_regions.iter().copied());
    }

    /// The expression `value` names: ANF's `(let [t e] t)` is `e`.
    fn unname<'h>(&self, value: &'h Hir) -> &'h Hir {
        match &value.kind {
            HirKind::Let { bindings, body } if self.names_its_init(bindings, body) => {
                self.unname(&bindings[0].1)
            }
            _ => value,
        }
    }

    /// Whether a `Let` only names its single init for its body: `(let [t e] t)`
    /// with `t` a compiler temporary.
    fn names_its_init(&self, bindings: &[(Binding, Hir)], body: &Hir) -> bool {
        match (bindings, &body.kind) {
            ([(t, _)], HirKind::Var(b)) => t == b && self.arena.get(*t).is_synthetic,
            _ => false,
        }
    }

    /// No user binding holds `region`: only compiler temporaries name the value.
    fn held_only_by_temps(&self, region: Region) -> bool {
        self.info
            .binding_source_regions
            .iter()
            .all(|(b, rs)| !rs.contains(&region) || self.arena.get(*b).is_synthetic)
    }

    /// Whether `region`'s value is stored anywhere but the push at `push`.
    fn stored_elsewhere(&self, region: Region, push: HirId) -> bool {
        self.info
            .cross_region_refs
            .iter()
            .any(|&(_, src, _)| src == region)
            || self
                .info
                .containment_edges
                .iter()
                .any(|&(site, src, _)| src == region && site != push)
    }

    /// The primitive `func` names, under the unshadowed-immutable guard the walk
    /// uses for every classification.
    fn primitive(&self, func: &Hir) -> Option<SymbolId> {
        let HirKind::Var(b) = &func.kind else {
            return None;
        };
        let info = self.arena.get(*b);
        (info.is_immutable && !info.is_mutated).then_some(info.name)
    }

    fn primitive_effect(&self, func: &Hir) -> Option<crate::primitives::def::RegionEffect> {
        self.primitive(func)
            .and_then(|name| self.call_class.effects.get(&name).copied())
    }
}

/// Whether an inline intrinsic reads its operands and keeps none of them:
/// a length, an element read, a type test, a comparison, or a copy.
fn reads_only(op: IntrinsicOp) -> bool {
    use IntrinsicOp::*;
    matches!(
        op,
        Length
            | Get
            | First
            | Rest
            | Has
            | TypeOf
            | IsNil
            | IsEmpty
            | IsBool
            | IsInt
            | IsFloat
            | IsString
            | IsKeyword
            | IsSymbol
            | IsPair
            | IsArray
            | IsStruct
            | IsSet
            | IsBytes
            | IsBox
            | IsClosure
            | IsFiber
            | Eq
            | Ne
            | Identical
            | Freeze
            | Thaw
    )
}
