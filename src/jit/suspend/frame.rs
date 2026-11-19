// audited: 2026-11-18
//! The interpreter frame a compiled side exit rebuilds out of its closure and the spill buffer.
//!
//! docs/impl/jit-closure.md

use crate::value::Value;

/// The spill counts a side-exit's metadata records: the buffer holds
/// `[params(num_params), locals(num_locals), operands(num_operands)]`.
pub(super) struct SpillCounts {
    pub(super) num_params: usize,
    pub(super) num_locals: usize,
    pub(super) num_operands: usize,
}

/// Rebuild the env and the stack the interpreter would hold for this frame,
/// out of the closure's captures and the spill buffer, in the layout
/// docs/impl/jit-closure.md gives.
///
/// The env holds the captures, the params, then one slot per local past the
/// params. A local whose bit `capture_locals_mask` sets moves its cell to
/// that env slot and leaves nil in its stack slot; every other local keeps
/// its value on the stack and nil in the env. Cells are first-class values in
/// compiled registers, so a spilled cell is the original object.
///
/// `spilled_values` must point to `num_params + num_locals + num_operands`
/// contiguous `Value`s, or be null when that sum is 0.
pub(super) fn rebuild_frame(
    closure: &crate::value::Closure,
    spilled_values: *const Value,
    counts: SpillCounts,
) -> (std::rc::Rc<Vec<Value>>, Vec<Value>) {
    let SpillCounts {
        num_params,
        num_locals,
        num_operands,
    } = counts;
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

    let past_params = num_locals.saturating_sub(num_params);
    let mut env = Vec::with_capacity(closure.env.len() + num_params + past_params);
    env.extend(closure.env.iter().copied());
    env.extend_from_slice(params);
    env.extend((num_params..num_locals).map(|l| if celled(l) { locals[l] } else { Value::NIL }));

    let mut stack = Vec::with_capacity(num_locals + num_operands);
    stack.extend(
        locals
            .iter()
            .enumerate()
            .map(|(l, v)| if celled(l) { Value::NIL } else { *v }),
    );
    stack.extend_from_slice(operands);
    (std::rc::Rc::new(env), stack)
}
