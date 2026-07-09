use super::*;

// Re-export the value-repr names that `linker.rs` imports for this module's
// use, so the `closure` submodule can reach them as `super::…` and `linker.rs`
// keeps consuming its own imports (otherwise they'd read as unused).
pub(super) use super::{Value, TAG_HEAP_START};

// Host-function registrations are split by concern; each submodule's `register`
// installs one cohesive family of `elle::rt_*` imports. `create_linker` calls
// them in a fixed order — the registration order itself is behavior-preserving
// (Wasmtime resolves imports by name), but keeping it stable keeps diffs and
// debugging predictable.
mod call;
mod closure;
mod fiber;
mod params;
mod tailcall;

/// Register host functions and return a Linker.
pub fn create_linker(engine: &Engine) -> Result<Linker<ElleHost>> {
    let mut linker = Linker::new(engine);

    call::register(&mut linker)?;
    closure::register(&mut linker)?;
    params::register(&mut linker)?;
    tailcall::register(&mut linker)?;
    fiber::register(&mut linker)?;

    Ok(linker)
}
