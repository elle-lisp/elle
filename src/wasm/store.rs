//! Wasmtime Engine/Store/Linker setup.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

/// Disk-backed compilation cache for wasmtime incremental compilation.
/// Cache entries are stored as files named by hex-encoded key hash.
#[derive(Debug)]
pub struct DiskCache(std::path::PathBuf);

impl DiskCache {
    pub fn new(path: std::path::PathBuf) -> Self {
        DiskCache(path)
    }
}

impl wasmtime::CacheStore for DiskCache {
    fn get(&self, key: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
        let path = self.0.join(hex_name(key));
        std::fs::read(&path).ok().map(std::borrow::Cow::Owned)
    }

    fn insert(&self, key: &[u8], value: Vec<u8>) -> bool {
        let path = self.0.join(hex_name(key));
        std::fs::write(&path, &value).is_ok()
    }
}

fn hex_name(key: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(key.len() * 2);
    for b in key {
        write!(s, "{:02x}", b).ok();
    }
    s
}

/// Create a Wasmtime Engine with tail-call support.
///
/// Honors `ELLE_JIT`:
///   - unset or non-zero: aggressive cranelift optimization (OptLevel::Speed)
///   - "0": cranelift optimization disabled (OptLevel::None) for faster compile
pub fn create_engine() -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_tail_call(true);
    config.wasm_multi_value(true);

    let jit_disabled = std::env::var("ELLE_JIT").map(|v| v == "0").unwrap_or(false);
    if jit_disabled {
        config.cranelift_opt_level(OptLevel::None);
    } else {
        config.cranelift_opt_level(OptLevel::Speed);
    }

    // Disk-backed compilation cache: reuses compiled machine code across runs.
    // Keyed on WASM bytecode content, so stdlib compilation is amortized.
    if let Ok(cache_dir) = std::env::var("ELLE_WASM_CACHE") {
        let path = std::path::PathBuf::from(cache_dir);
        std::fs::create_dir_all(&path).ok();
        let cache = DiskCache(path);
        config
            .enable_incremental_compilation(std::sync::Arc::new(cache))
            .ok();
    }

    Engine::new(&config)
}

/// Create a Store with ElleHost state and pre-loaded constant pool.
pub fn create_store(
    engine: &Engine,
    const_pool: Vec<Value>,
    closure_bytecodes: Vec<super::host::ClosureBytecode>,
) -> Store<ElleHost> {
    let mut host = ElleHost::new();

    // Pre-load heap constants into handle table and build a mapping from
    // const pool index → handle index. Immediate values (symbols, keywords,
    // etc.) are NOT inserted into the handle table, so pool indices and
    // handle indices diverge when the pool contains a mix of types.
    let mut pool_to_handle = Vec::with_capacity(const_pool.len());
    for value in &const_pool {
        if value.tag >= TAG_HEAP_START {
            let handle = host.handles.insert(*value);
            pool_to_handle.push(handle);
        } else {
            pool_to_handle.push(0); // unused for immediates
        }
    }

    host.const_pool = const_pool;
    host.pool_to_handle = pool_to_handle;
    host.closure_bytecodes = closure_bytecodes;
    Store::new(engine, host)
}

/// Build a WASM closure's environment in linear memory at `env_base`.
///
/// Layout: `[captures...][params...][local_slots...]`, each slot 16 bytes.
/// Handles varargs, LBox wrapping, and memory growth.
/// Updates `env_stack_ptr` to point past the new env region.
///
/// Generic over host type: works with both `ElleHost` (full-module) and
/// `TieredHost` (per-closure) via the `WasmEnvHost` trait.
pub fn prepare_wasm_env<T: super::host::WasmEnvHost>(
    caller: &mut Caller<'_, T>,
    closure: &std::rc::Rc<crate::value::closure::Closure>,
    args: &[Value],
    env_base: usize,
) {
    let template = &closure.template;
    let num_captures = template.num_captures;
    let num_params = template.num_params;
    let num_locals = template.num_locals;
    let lbox_params_mask = template.lbox_params_mask;
    let lbox_locals_mask = template.lbox_locals_mask;

    // Handle varargs: if arity is AtLeast(n), collect extra args
    // into a list (or array) and pass as the last parameter.
    let effective_args;
    let args = match template.arity {
        crate::value::types::Arity::AtLeast(required) => {
            let mut collected = Vec::with_capacity(num_params);
            for arg in args.iter().take(required) {
                collected.push(*arg);
            }
            let rest: Vec<Value> = args[required..].to_vec();
            let vararg_val = match template.vararg_kind {
                crate::hir::VarargKind::List => {
                    let mut list = Value::EMPTY_LIST;
                    for v in rest.iter().rev() {
                        list = Value::cons(*v, list);
                    }
                    list
                }
                _ => Value::array_mut(rest),
            };
            collected.push(vararg_val);
            while collected.len() < num_params {
                collected.push(Value::NIL);
            }
            effective_args = collected;
            effective_args.as_slice()
        }
        _ => args,
    };

    let extra_locals = num_locals.saturating_sub(num_params);
    let total_slots = num_captures + num_params + extra_locals;
    caller
        .data_mut()
        .set_env_stack_ptr(env_base + total_slots * 16);

    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("prepare_wasm_env: no memory");

    let needed_bytes = env_base + total_slots * 16;
    let current_bytes = memory.data_size(&*caller);
    if needed_bytes > current_bytes {
        let pages_needed = (needed_bytes - current_bytes).div_ceil(65536) as u64;
        memory
            .grow(&mut *caller, pages_needed)
            .expect("prepare_wasm_env: failed to grow memory");
    }

    for (i, val) in closure.env.iter().enumerate() {
        let (tag, payload) = caller.data_mut().value_to_wasm(*val);
        let offset = env_base + i * 16;
        let data = memory.data_mut(&mut *caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if i < 64 && lbox_params_mask & (1u64 << i) != 0 {
            Value::local_lbox(*arg)
        } else {
            *arg
        };
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    for i in args.len()..num_params {
        let val = if i < 64 && lbox_params_mask & (1u64 << i) != 0 {
            Value::local_lbox(Value::NIL)
        } else {
            Value::NIL
        };
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    for i in 0..extra_locals {
        let val = if i < 64 && lbox_locals_mask & (1u64 << i) != 0 {
            Value::local_lbox(Value::NIL)
        } else {
            Value::NIL
        };
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + num_params + i) * 16;
        let data = memory.data_mut(&mut *caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }
}

/// Handle the result of a WASM closure call (shared by call and resume paths).
///
/// If the function suspended (status > 0): snapshot env to the front frame,
/// clear memory[0..4], restore env_stack_ptr, return SIG_YIELD.
/// If normal return: read signal from memory[0..4], clear if non-zero, return.
/// If error: restore env_stack_ptr, return error.
pub(super) fn handle_wasm_result(
    caller: &mut Caller<'_, ElleHost>,
    call_result: std::result::Result<(), wasmtime::Error>,
    results: &[Val; 3],
    env_base: usize,
    label: &str,
) -> (i64, i64, i32) {
    match call_result {
        Ok(()) => {
            let tag = results[0].unwrap_i64();
            let payload = results[1].unwrap_i64();
            let status = results[2].unwrap_i32();

            if status > 0 {
                // Suspended: snapshot env and update the front frame.
                let env_end = caller.data().env_stack_ptr;
                let env_snapshot = if env_end > env_base {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("handle_wasm_result: no memory");
                    memory.data(&*caller)[env_base..env_end].to_vec()
                } else {
                    Vec::new()
                };

                // Update the most recently pushed frame (at back). rt_yield
                // pushes to the back; during a resume the old frame is still at
                // the front, so first_suspension_frame_mut() would return the
                // wrong frame.
                if let Some(frame) = caller.data_mut().back_suspension_frame_mut() {
                    frame.env_base = env_base;
                    frame.env_snapshot = env_snapshot;
                }

                // Clear signal word so callers don't see stale SIG_YIELD
                let memory = caller
                    .get_export("__elle_memory")
                    .and_then(|e| e.into_memory())
                    .expect("handle_wasm_result: no memory");
                if caller.data().debug {
                    let old = i32::from_le_bytes(memory.data(&*caller)[0..4].try_into().unwrap());
                    eprintln!("[{}] clearing memory[0..4] from {} to 0", label, old);
                }
                memory.data_mut(&mut *caller)[0..4].copy_from_slice(&0i32.to_le_bytes());

                caller.data_mut().env_stack_ptr = env_base;

                if caller.data().debug {
                    eprintln!(
                        "[{}] SUSPENDED: status={} tag={} payload={}",
                        label, status, tag, payload
                    );
                }

                (tag, payload, crate::value::fiber::SIG_YIELD.raw() as i32)
            } else {
                caller.data_mut().env_stack_ptr = env_base;

                let mut signal = {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("handle_wasm_result: no memory");
                    let data = memory.data(&*caller);
                    i32::from_le_bytes(data[0..4].try_into().unwrap())
                };
                if signal != 0 {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("handle_wasm_result: no memory");
                    memory.data_mut(&mut *caller)[0..4].copy_from_slice(&0i32.to_le_bytes());
                }
                // If a NativeFn tail call returned SIG_IO (written to
                // memory[0..4] by rt_prepare_tail_call), convert it to
                // SIG_YIELD so the WASM caller does yield-through instead
                // of treating it as an error-like early return. The I/O
                // request is in the return value; the fiber scheduler
                // will check fiber/bits for SIG_IO and drive the I/O.
                if signal as u32 & crate::signals::SIG_IO.raw() != 0 {
                    signal = crate::value::fiber::SIG_YIELD.raw() as i32;
                }

                if caller.data().debug {
                    let v = caller.data().wasm_to_value(tag, payload);
                    eprintln!(
                        "[{}] returned: tag={} payload={} signal={} = {:?}",
                        label, tag, payload, signal, v
                    );
                }
                (tag, payload, signal)
            }
        }
        Err(e) => {
            caller.data_mut().env_stack_ptr = env_base;
            let err = crate::value::error_val("exec-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, 1)
        }
    }
}

/// Call a WASM closure: build env in linear memory and invoke via table.
///
/// Each call allocates a fresh env region from `ElleHost::env_stack_ptr`
/// so that nested closure calls (recursion, higher-order) don't overwrite
/// each other's environments.
pub(super) fn call_wasm_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &std::rc::Rc<crate::value::closure::Closure>,
    wasm_idx: u32,
    args: &[Value],
) -> (i64, i64, i32) {
    let env_base = caller.data().env_stack_ptr;
    prepare_wasm_env(caller, closure, args, env_base);

    let table = caller
        .get_export("__elle_table")
        .and_then(|e| e.into_table())
        .expect("call_wasm_closure: no table");
    let func_ref = table
        .get(&mut *caller, wasm_idx as u64)
        .expect("call_wasm_closure: table index out of bounds");
    let func = func_ref
        .unwrap_func()
        .expect("call_wasm_closure: table entry is not a function");

    let mut results = [Val::I64(0), Val::I64(0), Val::I32(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0),
            Val::I32(0),
            Val::I32(0),
        ],
        &mut results,
    );

    handle_wasm_result(caller, call_result, &results, env_base, "call_wasm_closure")
}

/// Resume a suspended WASM closure with a resume value.
///
/// Pops the outermost suspension frame, restores its env to linear memory,
/// sets the resume value, and re-invokes the WASM function with
/// `ctx = resume_state`. If the function suspends again, the new frame
/// is saved. If it returns normally, returns the result.
///
/// For multi-frame suspension chains (yield-through-call), the caller
/// must call this repeatedly: first resume the innermost callee, then
/// use its result to resume the next frame.
pub fn resume_wasm_closure(
    caller: &mut Caller<'_, ElleHost>,
    resume_val: Value,
) -> Option<(i64, i64, i32)> {
    // Peek the front frame (innermost). During the WASM call, rt_load_saved_reg
    // reads from it. New frames pushed by rt_yield go to the back, so they
    // don't interfere. We pop_front AFTER the call completes.
    let frame = caller.data().first_suspension_frame()?;
    let wasm_func_idx = frame.wasm_func_idx;
    let resume_state = frame.resume_state;
    let env_base = frame.env_base;
    let env_snapshot = frame.env_snapshot.clone();

    // Set resume value for rt_get_resume_value
    let (resume_tag, resume_pay) = caller.data_mut().value_to_wasm(resume_val);
    caller.data_mut().resume_value = Some((resume_tag, resume_pay));

    // Restore env to linear memory
    if !env_snapshot.is_empty() {
        let memory = caller
            .get_export("__elle_memory")
            .and_then(|e| e.into_memory())
            .expect("resume_wasm_closure: no memory");

        let needed = env_base + env_snapshot.len();
        let current = memory.data_size(&*caller);
        if needed > current {
            let pages = (needed - current).div_ceil(65536) as u64;
            memory
                .grow(&mut *caller, pages)
                .expect("resume_wasm_closure: failed to grow memory");
        }

        let data = memory.data_mut(&mut *caller);
        data[env_base..env_base + env_snapshot.len()].copy_from_slice(&env_snapshot);
    }

    caller.data_mut().env_stack_ptr = env_base + env_snapshot.len();

    if caller.data().debug {
        eprintln!(
            "[resume_wasm_closure] env_base={} env_size={} resume_state={} wasm_func_idx={}",
            env_base,
            env_snapshot.len(),
            resume_state,
            wasm_func_idx
        );
        if !env_snapshot.is_empty() {
            let mut slots = Vec::new();
            let num_slots = env_snapshot.len() / 16;
            for i in 0..num_slots.min(4) {
                let off = i * 16;
                let tag = i64::from_le_bytes(env_snapshot[off..off + 8].try_into().unwrap());
                let pay = i64::from_le_bytes(env_snapshot[off + 8..off + 16].try_into().unwrap());
                slots.push(format!("({},{})", tag, pay));
            }
            eprintln!("[resume_wasm_closure] env slots: {:?}", slots);
        }
    }

    // Look up the WASM function in the table
    let table = caller
        .get_export("__elle_table")
        .and_then(|e| e.into_table())
        .expect("resume_wasm_closure: no table");
    let func_ref = table
        .get(&mut *caller, wasm_func_idx as u64)
        .expect("resume_wasm_closure: table index out of bounds");
    let func = func_ref
        .unwrap_func()
        .expect("resume_wasm_closure: table entry is not a function");

    let mut results = [Val::I64(0), Val::I64(0), Val::I32(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0),
            Val::I32(0),
            Val::I32(resume_state as i32),
        ],
        &mut results,
    );

    // Pop the front frame now that the call is done. If the function yielded
    // again, rt_yield pushed new frames to the back — they survive this pop.
    caller.data_mut().pop_suspension_frame();
    caller.data_mut().resume_value = None;

    let (t, p, s) = handle_wasm_result(
        caller,
        call_result,
        &results,
        env_base,
        "resume_wasm_closure",
    );
    Some((t, p, s))
}

/// Compile WASM bytes into a Module.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<Module> {
    Module::new(engine, wasm_bytes)
}

/// Instantiate a module and call its entry function.
/// If the entry function suspends (e.g. I/O inside ev/run), drive it
/// to completion by processing I/O inline via SyncBackend and resuming.
pub fn run_module(
    linker: &Linker<ElleHost>,
    store: &mut Store<ElleHost>,
    module: &Module,
) -> Result<Value> {
    use crate::io::request::IoRequest;
    use crate::signals::SIG_IO;

    let instance = linker.instantiate(&mut *store, module)?;
    let entry = instance.get_typed_func::<(i32,), (i64, i64, i32)>(&mut *store, "__elle_entry")?;
    let (mut tag, mut payload, mut status) = entry.call(&mut *store, (0,))?;

    // The entry function may suspend when ev/run's scheduler does I/O
    // (SIG_IO propagates through yield-through-call to the top level).
    // Drive it to completion by executing I/O inline and re-calling the
    // entry function with the resume state from its outermost frame.
    while status > 0 {
        let value = store.data().wasm_to_value(tag, payload);

        // Execute I/O if the innermost frame has SIG_IO
        let resume_val = if let Some(frame) = store.data().first_suspension_frame() {
            if frame.signal_bits & SIG_IO.raw() != 0 {
                if let Some(request) = value.as_external::<IoRequest>() {
                    let (_bits, result) = store.data_mut().execute_io_inline(request);
                    result
                } else {
                    value
                }
            } else {
                value
            }
        } else {
            break;
        };

        // Drain all suspension frames. The outermost (last) frame has the
        // entry function's resume_state; inner frames are discarded because
        // the entry function's CPS will re-create them on re-entry.
        let mut resume_state = 0i32;
        while store.data().has_suspension_frames() {
            if let Some(frame) = store.data_mut().pop_suspension_frame() {
                resume_state = frame.resume_state as i32;
            }
        }

        let (resume_tag, resume_pay) = store.data_mut().value_to_wasm(resume_val);
        store.data_mut().resume_value = Some((resume_tag, resume_pay));

        let (t, p, s) = entry.call(&mut *store, (resume_state,))?;
        store.data_mut().resume_value = None;
        tag = t;
        payload = p;
        status = s;
    }

    let value = store.data().wasm_to_value(tag, payload);
    Ok(value)
}
