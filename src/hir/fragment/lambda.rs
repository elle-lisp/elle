// audited: 2026-10-06
//! A function closed into fragments, one per arity its body dispatches on, for a graft or a fused loop.
//!
//! docs/impl/graft.md
//! docs/impl/hir.md

use super::{ClosedGlobals, HirFragment};
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind, Lambda, VarargKind};
use crate::hir::pattern::HirPattern;
use crate::value::SymbolId;

/// A function's body, closed so another unit or another call site can take a
/// copy of it. The function's template carries one, and the unit that defines
/// the function reads the same one off its lambda node.
///
/// A body that dispatches on its rest list with `(match rest …)` closes arm by
/// arm, because a call's operand count decides the arm before the call runs.
/// An arm that does not close leaves the calls it would serve as calls.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LambdaFragment {
    /// The name the function was bound to where it was defined. An arm that
    /// calls this name is recursive, and never grafts.
    name: Option<SymbolId>,
    /// How many fixed parameters precede the rest list.
    fixed: usize,
    variadic: bool,
    /// The function's `(numeric!)` declaration.
    numeric: bool,
    /// Does the body prove it returns an array? Asked only by a fused
    /// `mapcat`, and recorded here because the proof reads the defining unit.
    returns_array: bool,
    arms: Vec<Arm>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Arm {
    shape: Shape,
    /// `None` when this arm's body does not close.
    body: Option<ArmBody>,
}

/// Which counts of extra operands an arm accepts.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
enum Shape {
    /// The whole body: every count, with the rest list bound to them all.
    Whole,
    /// A list pattern of plain variables: exactly `elements` extra operands,
    /// or at least that many when it ends in a `& tail`.
    List { elements: usize, tail: bool },
    /// `_` or a bare variable: every count.
    Any,
    /// A pattern or a guard nothing can decide before the call runs. A call
    /// that reaches this arm stays a call.
    Undecidable,
}

/// One arm's body and the fragment indices of what a call binds for it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArmBody {
    pub fragment: HirFragment,
    /// The fixed parameters, in order.
    pub params: Vec<u32>,
    /// The rest parameter, bound to the list of the extra operands.
    pub rest: Option<u32>,
    /// Each list-pattern element: its variable, or `None` for `_`.
    pub elements: Vec<Option<u32>>,
    /// The `& tail` variable, or the bare variable an `Any` arm binds: the list
    /// of the extra operands the elements do not take.
    pub tail: Option<u32>,
    /// Does this arm call the function's own name? A spliced call never
    /// grafts, so it cannot unroll and does not count: `(apply < b more)`
    /// expands to one.
    pub recursive: bool,
}

/// The arm a call's operand count selects.
pub struct Selected<'a> {
    pub body: &'a ArmBody,
    /// Its position among the function's arms, which is also the position of
    /// its `ClosedGlobals` in what [`LambdaFragment::close`] returned.
    pub arm: usize,
}

impl LambdaFragment {
    /// Close `lambda` (a `Lambda` node) against its defining `arena`. `name` is
    /// the binding it initializes, if any. Returns the fragment and, per arm,
    /// the defining bindings its globals stand for.
    ///
    /// `None` for a function a call cannot be replaced by: one with `&opt`,
    /// `&keys` or `&named` parameters, a signal bound on a parameter, or a
    /// parameter the body mutates.
    pub fn close(
        lambda: &Hir,
        name: Option<SymbolId>,
        arena: &BindingArena,
        returns_array: bool,
    ) -> Option<(Self, Vec<ClosedGlobals>)> {
        let Lambda {
            params,
            num_required,
            rest_param,
            vararg_kind,
            body,
            param_bounds,
            assert_numeric,
            ..
        } = lambda.kind.as_lambda()?;
        let fixed: Vec<Binding> = params
            .iter()
            .copied()
            .filter(|p| Some(*p) != *rest_param)
            .collect();
        if *num_required != fixed.len()
            || !param_bounds.is_empty()
            || (rest_param.is_some() && *vararg_kind != VarargKind::List)
            || fixed
                .iter()
                .chain(rest_param.iter())
                .any(|p| arena.get(*p).is_mutated)
        {
            return None;
        }
        let closer = Closer {
            arena,
            fixed: &fixed,
            rest: *rest_param,
            name,
        };
        let (arms, globals) = match rest_param.and_then(|r| rest_dispatch(body, r)) {
            Some(match_arms) => match_arms
                .iter()
                .map(|(p, g, b)| closer.arm(p, g, b))
                .unzip(),
            None => {
                let (body, globals) = closer.close(body, &[], None);
                (
                    vec![Arm {
                        shape: Shape::Whole,
                        body,
                    }],
                    vec![globals],
                )
            }
        };
        Some((
            LambdaFragment {
                name,
                fixed: fixed.len(),
                variadic: rest_param.is_some(),
                numeric: *assert_numeric,
                returns_array,
                arms,
            },
            globals,
        ))
    }

    /// The arm a call with `operands` operands runs, when that is decided
    /// before the call runs and the arm closes.
    pub fn select(&self, operands: usize) -> Option<Selected<'_>> {
        let extra = operands.checked_sub(self.fixed)?;
        if !self.variadic && extra != 0 {
            return None;
        }
        for (i, arm) in self.arms.iter().enumerate() {
            let takes = match arm.shape {
                Shape::Whole | Shape::Any => true,
                Shape::List { elements, tail } => {
                    if tail {
                        extra >= elements
                    } else {
                        extra == elements
                    }
                }
                Shape::Undecidable => return None,
            };
            if takes {
                return arm.body.as_ref().map(|body| Selected { body, arm: i });
            }
        }
        None
    }

    /// The one body of a function with fixed parameters and no rest list, for
    /// a fused loop to splice.
    pub fn whole(&self) -> Option<&ArmBody> {
        match self.arms.as_slice() {
            [Arm {
                shape: Shape::Whole,
                body: Some(body),
            }] if !self.variadic => Some(body),
            _ => None,
        }
    }

    /// Each arm's body, in order, `None` where the arm does not close.
    pub fn arm_bodies(&self) -> impl Iterator<Item = Option<&ArmBody>> {
        self.arms.iter().map(|a| a.body.as_ref())
    }

    pub fn fixed(&self) -> usize {
        self.fixed
    }

    pub fn numeric(&self) -> bool {
        self.numeric
    }

    pub fn returns_array(&self) -> bool {
        self.returns_array
    }

    pub fn name(&self) -> Option<SymbolId> {
        self.name
    }
}

/// The arms of a body that is a `match` on the rest parameter, looking through
/// a one-form `begin`.
fn rest_dispatch(body: &Hir, rest: Binding) -> Option<&[(HirPattern, Option<Hir>, Hir)]> {
    match &body.kind {
        HirKind::Begin(forms) if forms.len() == 1 => rest_dispatch(&forms[0], rest),
        HirKind::Match { value, arms } if matches!(value.kind, HirKind::Var(b) if b == rest) => {
            Some(arms)
        }
        _ => None,
    }
}

/// What every arm of one function shares when it closes.
struct Closer<'a> {
    arena: &'a BindingArena,
    fixed: &'a [Binding],
    rest: Option<Binding>,
    name: Option<SymbolId>,
}

impl Closer<'_> {
    /// Close one arm of a rest-list dispatch.
    fn arm(&self, pat: &HirPattern, guard: &Option<Hir>, body: &Hir) -> (Arm, ClosedGlobals) {
        let undecidable = || {
            (
                Arm {
                    shape: Shape::Undecidable,
                    body: None,
                },
                Vec::new(),
            )
        };
        if guard.is_some() {
            return undecidable();
        }
        let (shape, elements, tail) = match pat {
            HirPattern::Wildcard => (Shape::Any, Vec::new(), None),
            HirPattern::Var(v) => (Shape::Any, Vec::new(), Some(*v)),
            HirPattern::List { elements, rest } => {
                let Some(vars) = elements
                    .iter()
                    .map(plain_variable)
                    .collect::<Option<Vec<_>>>()
                else {
                    return undecidable();
                };
                let tail = match rest.as_deref() {
                    None => None,
                    Some(p) => match plain_variable(p) {
                        Some(v) => v,
                        None => return undecidable(),
                    },
                };
                let shape = Shape::List {
                    elements: vars.len(),
                    tail: rest.is_some(),
                };
                (shape, vars, tail)
            }
            _ => return undecidable(),
        };
        let (body, globals) = self.close(body, &elements, tail);
        (Arm { shape, body }, globals)
    }

    /// Close `body` with the fixed parameters, the rest parameter, the list
    /// elements' variables and the tail variable introduced in that order.
    fn close(
        &self,
        body: &Hir,
        elements: &[Option<Binding>],
        tail: Option<Binding>,
    ) -> (Option<ArmBody>, ClosedGlobals) {
        let mut introduced: Vec<Binding> = self.fixed.to_vec();
        introduced.extend(self.rest);
        introduced.extend(elements.iter().flatten());
        introduced.extend(tail);
        let Some((index, fragment, globals)) =
            HirFragment::close_with_globals(body, &introduced, self.arena, true)
        else {
            return (None, Vec::new());
        };
        let mut index = index.into_iter();
        let params = index.by_ref().take(self.fixed.len()).collect();
        let rest = self.rest.and_then(|_| index.next());
        let elements = elements
            .iter()
            .map(|e| e.and_then(|_| index.next()))
            .collect();
        let tail = tail.and_then(|_| index.next());
        let recursive = self.name.is_some_and(|n| {
            fragment
                .called_globals()
                .iter()
                .any(|(_, callee)| *callee == n)
        });
        (
            Some(ArmBody {
                fragment,
                params,
                rest,
                elements,
                tail,
                recursive,
            }),
            globals,
        )
    }
}

/// `Some(Some(v))` for a variable, `Some(None)` for `_`, `None` for any other
/// pattern.
fn plain_variable(p: &HirPattern) -> Option<Option<Binding>> {
    match p {
        HirPattern::Var(v) => Some(Some(*v)),
        HirPattern::Wildcard => Some(None),
        _ => None,
    }
}
