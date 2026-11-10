// audited: 2026-11-10
//! Which of the unit's own bindings a grafted body may name where the graft lands, and the captures it adds to name the rest.
//!
//! docs/impl/graft.md
//! docs/impl/graft/captures.md

use crate::hir::arena::BindingArena;
use crate::hir::binding::{Binding, CaptureInfo, CaptureKind};
use rustc_hash::{FxHashMap, FxHashSet};

/// The bindings reachable at the walk's current position, one frame per
/// enclosing lambda.
///
/// A body grafted from a function of this unit names that function's globals
/// directly. Inside a lambda, a module binding is reachable through the
/// lambda's captures, which the analyzer fixed before the graft ran, or
/// through a capture the graft adds: one on this lambda and on each enclosing
/// lambda down to the one whose frame holds the binding's slot or cell.
pub(super) struct Scope {
    frames: Vec<Frame>,
}

#[derive(Default)]
struct Frame {
    /// What the lambda captures, with the kind the analyzer or a graft gave it.
    captures: FxHashMap<Binding, CaptureKind>,
    /// Bindings introduced in this frame and initialized by now, counted
    /// because a binding may be introduced again by an inner form.
    bound: FxHashMap<Binding, u32>,
    /// Module bindings a `letrec` of this frame introduces, whose cells exist
    /// before any of its initializers runs. A lambda built from inside this
    /// frame captures such a cell whether or not its initializer has run.
    prebound: FxHashSet<Binding>,
    /// The captures grafts below added to this lambda, in order.
    added: Vec<Binding>,
    /// The self-references of this lambda a graft below turned into plain
    /// captures, because a lambda inside it needs the cell.
    reclassified: Vec<Binding>,
}

/// What a lambda's frame accumulated while the walk was inside it.
pub(super) struct Added {
    pub(super) captures: Vec<Binding>,
    pub(super) reclassified: Vec<Binding>,
}

impl Scope {
    pub(super) fn new() -> Self {
        Scope {
            frames: vec![Frame::default()],
        }
    }

    pub(super) fn enter_lambda(&mut self, captures: &[CaptureInfo], params: &[Binding]) {
        let mut frame = Frame {
            captures: captures.iter().map(|c| (c.binding, c.kind)).collect(),
            ..Frame::default()
        };
        for p in params {
            *frame.bound.entry(*p).or_default() += 1;
        }
        self.frames.push(frame);
    }

    /// Leave the lambda, answering the captures grafts inside it added.
    pub(super) fn leave_lambda(&mut self) -> Added {
        let frame = self.frames.pop().expect("a lambda frame is left once");
        Added {
            captures: frame.added,
            reclassified: frame.reclassified,
        }
    }

    pub(super) fn bind(&mut self, b: Binding) {
        *self.frame().bound.entry(b).or_default() += 1;
    }

    pub(super) fn unbind(&mut self, b: Binding) {
        let bound = &mut self.frame().bound;
        if let Some(n) = bound.get_mut(&b) {
            *n -= 1;
            if *n == 0 {
                bound.remove(&b);
            }
        }
    }

    /// Record the module bindings a `letrec` of this frame introduces, ahead
    /// of its initializers.
    pub(super) fn prebind(&mut self, bindings: impl IntoIterator<Item = Binding>) {
        self.frame().prebound.extend(bindings);
    }

    /// May code at this position name `b`? A primitive is a constant anywhere.
    /// A module binding is named in the frame that introduces it only once its
    /// initializer ran, because the grafted body reads the cell at once.
    pub(super) fn reaches(&self, b: Binding, arena: &BindingArena) -> bool {
        if arena.get(b).is_primitive {
            return true;
        }
        let frame = self
            .frames
            .last()
            .expect("the outermost frame is never left");
        frame.captures.contains_key(&b) || frame.bound.contains_key(&b)
    }

    /// May the graft add a capture of `b` to the lambdas the walk is inside?
    /// It may when some enclosing frame holds `b`: as a capture, as a binding
    /// initialized by now, or as a cell a `letrec` of that frame minted ahead
    /// of its initializers.
    pub(super) fn can_capture(&self, b: Binding) -> bool {
        let inner = self.frames.len() - 1;
        self.frames[..inner].iter().any(|f| f.holds(b))
    }

    /// Add a capture of `b` to the current lambda and to each enclosing lambda
    /// down to the frame that holds it. A lambda on the way that holds `b` as a
    /// self-reference holds the executing closure and not the cell, so that
    /// capture becomes a plain one and the walk goes on to the frame below.
    pub(super) fn capture(&mut self, b: Binding) {
        for frame in self.frames.iter_mut().rev() {
            if let Some(kind) = frame.captures.get_mut(&b) {
                if !matches!(kind, CaptureKind::Recursive { .. }) {
                    return;
                }
                *kind = CaptureKind::Local;
                frame.reclassified.push(b);
                continue;
            }
            if frame.holds(b) {
                return;
            }
            frame.captures.insert(b, CaptureKind::Local);
            frame.added.push(b);
        }
        unreachable!("can_capture found a frame that holds the binding");
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames
            .last_mut()
            .expect("the outermost frame is never left")
    }
}

impl Frame {
    /// Does this frame hold `b`'s slot or cell, or a capture of it?
    fn holds(&self, b: Binding) -> bool {
        self.captures.contains_key(&b) || self.bound.contains_key(&b) || self.prebound.contains(&b)
    }
}
