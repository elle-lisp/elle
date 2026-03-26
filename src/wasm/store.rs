//! Wasmtime Engine/Store/Linker setup.
//!
//! Creates the Wasmtime runtime environment, registers host functions,
//! and provides module compilation and execution.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

/// Create a Wasmtime Engine with tail-call support.
pub fn create_engine() -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_tail_call(true);
    config.wasm_multi_value(true);
    Engine::new(&config)
}

/// Create a Store with ElleHost state.
pub fn create_store(engine: &Engine) -> Store<ElleHost> {
    Store::new(engine, ElleHost::new())
}

/// Register host functions and return a Linker.
///
/// Currently registers:
/// - `elle::call_primitive` — dispatch to Elle primitives
pub fn create_linker(engine: &Engine) -> Result<Linker<ElleHost>> {
    let mut linker = Linker::new(engine);

    // call_primitive(prim_id: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i32)
    //
    // For Phase 0, args are passed via a simplified protocol:
    // args_ptr points into linear memory where args are laid out as
    // [tag_0: i64, payload_0: i64, tag_1: i64, payload_1: i64, ...]
    linker.func_wrap(
        "elle",
        "call_primitive",
        |mut caller: Caller<'_, ElleHost>,
         prim_id: i32,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            let memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory());

            let args = if let Some(memory) = memory {
                let data = memory.data(&caller);
                let mut args = Vec::with_capacity(nargs as usize);
                for i in 0..nargs as usize {
                    let offset = args_ptr as usize + i * 16;
                    let tag =
                        i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as u64;
                    let payload =
                        i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap())
                            as u64;
                    let value = if tag < TAG_HEAP_START {
                        Value { tag, payload }
                    } else {
                        caller.data().handles.get(payload)
                    };
                    args.push(value);
                }
                args
            } else {
                Vec::new()
            };

            let (bits, result) = caller.data_mut().call_primitive(prim_id as u32, &args);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.0 as i32)
        },
    )?;

    Ok(linker)
}

/// Compile WASM bytes into a Module.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<Module> {
    Module::new(engine, wasm_bytes)
}

/// Instantiate a module and call its entry function.
///
/// Returns the result as a Value.
pub fn run_module(
    linker: &Linker<ElleHost>,
    store: &mut Store<ElleHost>,
    module: &Module,
) -> Result<Value> {
    let instance = linker.instantiate(&mut *store, module)?;

    let entry = instance.get_typed_func::<(), (i64, i64)>(&mut *store, "__elle_entry")?;

    let (tag, payload) = entry.call(&mut *store, ())?;
    let value = store.data().wasm_to_value(tag, payload);
    Ok(value)
}
