// audited: 2026-11-27
//! The interpreter frame a compiled side exit rebuilds out of its closure and the spill buffer.
//!
//! docs/impl/jit-closure.md

use crate::lir::code::{LiveSlots, SiteRef};
use crate::value::Value;

/// The buffer a side exit spills, as its site's metadata records it:
/// `[params, locals, operands]`, one `Value` each.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpillLayout {
    params: u16,
    locals: u16,
    operands: u16,
    /// The locals the exit stores, or `None` where it stores every one.
    kept: Option<LiveSlots>,
}

impl SpillLayout {
    /// The buffer `site` spills in a function of `num_params` parameters,
    /// storing the locals in `kept`, or every local where `kept` is `None`.
    pub(crate) fn of(site: SiteRef<'_>, num_params: usize, kept: Option<LiveSlots>) -> Self {
        SpillLayout {
            params: num_params as u16,
            locals: site.num_locals,
            operands: site.stack_regs.len() as u16,
            kept,
        }
    }

    /// Whether the exit stores local `l`.
    fn keeps(&self, l: usize) -> bool {
        self.kept
            .as_ref()
            .is_none_or(|kept| kept.contains(l as u16))
    }
}

#[cfg(test)]
impl SpillLayout {
    /// This layout with `n` parameters.
    pub(crate) fn params(mut self, n: u16) -> Self {
        self.params = n;
        self
    }

    /// This layout with `n` local slots, each parameter's own slot included.
    pub(crate) fn locals(mut self, n: u16) -> Self {
        self.locals = n;
        self
    }

    /// This layout with `n` operands.
    pub(crate) fn operands(mut self, n: u16) -> Self {
        self.operands = n;
        self
    }

    /// This layout, storing the locals in `kept` alone.
    pub(crate) fn kept(mut self, kept: LiveSlots) -> Self {
        self.kept = Some(kept);
        self
    }
}

/// Rebuild the env and the stack the interpreter would hold for this frame,
/// out of the closure's captures and the spill buffer, in the layout
/// docs/impl/jit-closure.md gives.
///
/// The env holds the captures, the params, then one slot per local past the
/// params. A local whose bit `capture_locals_mask` sets moves its cell to
/// that env slot and leaves nil in its stack slot; every other local keeps
/// its value on the stack and nil in the env. Cells are first-class values in
/// compiled registers, so a spilled cell is the original object. A local the
/// exit did not store reads nil in both places: its buffer slot holds what an
/// earlier exit stored there.
///
/// `spilled_values` must point to the `Value`s `layout` names, contiguous, or
/// be null when it names none.
pub(super) fn rebuild_frame(
    closure: &crate::value::Closure,
    spilled_values: *const Value,
    layout: &SpillLayout,
) -> (std::rc::Rc<Vec<Value>>, Vec<Value>) {
    let num_params = layout.params as usize;
    let num_locals = layout.locals as usize;
    let num_operands = layout.operands as usize;
    let total = num_params + num_locals + num_operands;
    let spill: &[Value] = if total == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(spilled_values, total) }
    };
    let (params, rest) = spill.split_at(num_params);
    let (locals, operands) = rest.split_at(num_locals);
    let mask = closure.template.capture_locals_mask();
    let celled = |l: usize| l >= num_params && mask.is_set(l - num_params);
    let value = |l: usize| {
        if layout.keeps(l) {
            locals[l]
        } else {
            Value::NIL
        }
    };

    let past_params = num_locals.saturating_sub(num_params);
    let mut env = Vec::with_capacity(closure.env.len() + num_params + past_params);
    env.extend(closure.env.iter().copied());
    env.extend_from_slice(params);
    env.extend((num_params..num_locals).map(|l| if celled(l) { value(l) } else { Value::NIL }));

    let mut stack = Vec::with_capacity(num_locals + num_operands);
    stack.extend((0..num_locals).map(|l| if celled(l) { Value::NIL } else { value(l) }));
    stack.extend_from_slice(operands);
    (std::rc::Rc::new(env), stack)
}
