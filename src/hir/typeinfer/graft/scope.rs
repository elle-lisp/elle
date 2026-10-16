// audited: 2026-10-06
//! Which of the unit's own bindings a grafted body may name where the graft lands.
//!
//! docs/impl/graft.md

use crate::hir::arena::BindingArena;
use crate::hir::binding::{Binding, CaptureInfo};
use rustc_hash::{FxHashMap, FxHashSet};

/// The bindings reachable at the walk's current position, one frame per
/// enclosing lambda.
///
/// A body grafted from a function of this unit names that function's globals
/// directly. Inside a lambda, a module binding is reachable only through the
/// lambda's captures, and the analyzer fixed those before the graft ran. A
/// graft that named a binding the lambda does not capture would reach for a
/// slot the lowerer never allocated. So a graft asks here first, and declines.
pub(super) struct Scope {
    frames: Vec<Frame>,
}

#[derive(Default)]
struct Frame {
    captures: FxHashSet<Binding>,
    /// Bindings introduced in this frame and initialized by now, counted
    /// because a binding may be introduced again by an inner form.
    bound: FxHashMap<Binding, u32>,
}

impl Scope {
    pub(super) fn new() -> Self {
        Scope {
            frames: vec![Frame::default()],
        }
    }

    pub(super) fn enter_lambda(&mut self, captures: &[CaptureInfo], params: &[Binding]) {
        let mut frame = Frame {
            captures: captures.iter().map(|c| c.binding).collect(),
            bound: FxHashMap::default(),
        };
        for p in params {
            *frame.bound.entry(*p).or_default() += 1;
        }
        self.frames.push(frame);
    }

    pub(super) fn leave_lambda(&mut self) {
        self.frames.pop();
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

    /// May code at this position name `b`? A primitive is a constant anywhere.
    pub(super) fn reaches(&self, b: Binding, arena: &BindingArena) -> bool {
        if arena.get(b).is_primitive {
            return true;
        }
        let frame = self
            .frames
            .last()
            .expect("the outermost frame is never left");
        frame.captures.contains(&b) || frame.bound.contains_key(&b)
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames
            .last_mut()
            .expect("the outermost frame is never left")
    }
}
