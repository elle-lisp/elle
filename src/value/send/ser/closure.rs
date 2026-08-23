//! Serializing a live closure instance into the bundle's intern table.
//!
//! Split from the value-tag `match` because the closure arm is by far the
//! largest: it manages cycle detection (pre-inserting a placeholder before
//! recursing into `env`/constants/LIR) and rebuilds the full nested-lambda
//! blueprint tree. Isolating it keeps the tag dispatch readable.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use super::lir::convert_lir_for_send;
use super::template::sendable_from_template;

/// Serialize a closure instance reached at heap value `value`, interning it
/// into `ctx.closures` with cycle detection and returning a `Ref` to its slot.
///
/// `closure_rc` is the derefed closure payload. `value.payload` is the identity
/// key: for heap values, payload IS the pointer, so it uniquely names the
/// closure across the (possibly cyclic) graph.
pub(super) fn send_closure(
    value: Value,
    closure_rc: &crate::value::closure::Closure,
    ctx: &mut SerContext<'_>,
) -> Result<SendValue, String> {
    // Use value.payload as identity key — for heap values, payload IS the pointer.
    let key = value.payload;

    // Already visited → return Ref to existing intern entry.
    if let Some(&idx) = ctx.visited.get(&key) {
        return Ok(SendValue::Ref(idx));
    }

    // Reserve an index BEFORE recursing so back-references resolve to this entry.
    let idx = ctx.closures.len();
    // Push a placeholder (will be overwritten below).
    ctx.closures.push(SendableClosure {
        bytecode: Vec::new(),
        arity: closure_rc.template.arity,
        num_locals: 0,
        num_captures: 0,
        num_params: 0,
        constants: Vec::new(),
        signal: closure_rc.template.signal,
        capture_params_mask: 0,
        capture_locals_mask: crate::value::CaptureMask::empty(),
        symbol_names: HashMap::new(),
        location_map: LocationMap::new(),
        doc: None,
        vararg_kind: closure_rc.template.vararg_kind.clone(),
        name: None,
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir_function: None,
        lir_value_pool: Vec::new(),
        child_protos: Vec::new(),
        merged_slots: Vec::new(), // placeholder; replaced below
        frame_release_slots: closure_rc.template.frame_release_slots.to_vec(),
        frame_release_regions: closure_rc.template.frame_release_regions.to_vec(),
    });
    ctx.visited.insert(key, idx);

    // Serialize environment (may contain back-references to this closure via LBox).
    let env: Result<Vec<SendValue>, String> = closure_rc
        .env
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect();
    let env = env?;

    // Serialize constants.
    let constants: Result<Vec<SendValue>, String> = closure_rc
        .template
        .constants
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect();
    let constants = constants?;

    // Serialize doc (optional) — plain string data, not a heap Value.
    let doc = closure_rc.template.doc.as_deref().map(str::to_string);

    // Clone LIR for JIT in spawned threads. Strip doc (Value/Rc) and
    // syntax (Rc<Syntax>), then convert every cross-thread-unsafe
    // ValueConst: scalars inline, closures → ClosureRef, compounds →
    // ValueRef into `lir_value_pool` (serialized through `ctx` so nested
    // closures intern correctly). The LIR is preserved unconditionally —
    // a spawned closure keeps its JIT-able body across the boundary.
    let (lir_function, lir_value_pool) = match closure_rc.template.lir_function.as_ref() {
        Some(lir) => {
            let mut lir = (**lir).clone();
            lir.doc = None;
            lir.syntax = None;
            match convert_lir_for_send(&mut lir, ctx)? {
                Some(pool) => (Some(lir), pool),
                // A closure-valued ValueConst couldn't be interned — drop
                // the LIR (the closure still runs via bytecode in the worker).
                None => (None, Vec::new()),
            }
        }
        None => (None, Vec::new()),
    };

    // Serialize the nested-lambda blueprints so the worker's reconstructed
    // template carries them and `MakeClosure` resolves by index.
    let child_protos: Vec<SendableClosure> = closure_rc
        .template
        .child_protos
        .iter()
        .map(|p| sendable_from_template(p, ctx))
        .collect::<Result<_, _>>()?;

    // Replace placeholder with complete entry.
    ctx.closures[idx] = SendableClosure {
        bytecode: (*closure_rc.template.bytecode).clone(),
        arity: closure_rc.template.arity,
        num_locals: closure_rc.template.num_locals,
        num_captures: closure_rc.template.num_captures,
        num_params: closure_rc.template.num_params,
        constants,
        signal: closure_rc.template.signal,
        capture_params_mask: closure_rc.template.capture_params_mask,
        capture_locals_mask: closure_rc.template.capture_locals_mask.clone(),

        symbol_names: (*closure_rc.template.symbol_names).clone(),
        location_map: (*closure_rc.template.location_map).clone(),
        doc,
        vararg_kind: closure_rc.template.vararg_kind.clone(),
        name: closure_rc.template.name.as_ref().map(|s| s.to_string()),
        squelch_mask: closure_rc.squelch_mask,
        env,
        lir_function,
        lir_value_pool,
        child_protos,
        merged_slots: closure_rc.template.merged_slots.iter().copied().collect(),
        frame_release_slots: (*closure_rc.template.frame_release_slots).clone(),
        frame_release_regions: (*closure_rc.template.frame_release_regions).clone(),
    };

    Ok(SendValue::Ref(idx))
}
