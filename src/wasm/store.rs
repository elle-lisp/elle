//! Wasmtime Engine/Store/Linker setup.

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

/// Create a Store with ElleHost state and pre-loaded constant pool.
pub fn create_store(engine: &Engine, const_pool: Vec<Value>) -> Store<ElleHost> {
    let mut host = ElleHost::new();

    // Pre-load heap constants into handle table.
    // The const_pool index maps 1:1 to the order rt_load_const will be called.
    for value in &const_pool {
        if value.tag >= TAG_HEAP_START {
            host.handles.insert(*value);
        }
    }

    host.const_pool = const_pool;
    Store::new(engine, host)
}

/// Register host functions and return a Linker.
pub fn create_linker(engine: &Engine) -> Result<Linker<ElleHost>> {
    let mut linker = Linker::new(engine);

    // call_primitive(prim_id: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "call_primitive",
        |mut caller: Caller<'_, ElleHost>,
         prim_id: i32,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let (bits, result) = caller.data_mut().call_primitive(prim_id as u32, &args);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.0 as i32)
        },
    )?;

    // rt_call(func_tag: i64, func_payload: i64, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "rt_call",
        |mut caller: Caller<'_, ElleHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            // Resolve the function value
            let func_val = caller.data().wasm_to_value(func_tag, func_payload);

            // Read args from linear memory
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);

            // Dispatch based on function type
            if func_val.is_native_fn() {
                // NativeFn: extract function pointer, call directly
                let native_fn = func_val.as_native_fn().expect("rt_call: expected NativeFn");
                let (bits, result) = native_fn(&args);
                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                (tag, payload, bits.0 as i32)
            } else {
                // For now, unsupported function type → error
                let err = crate::value::error_val(
                    "type-error",
                    format!(
                        "rt_call: cannot call value of type {}",
                        func_val.type_name()
                    ),
                );
                let (tag, payload) = caller.data_mut().value_to_wasm(err);
                (tag, payload, 1) // SIG_ERROR = 1
            }
        },
    )?;

    // rt_load_const(index: i32) -> (tag: i64, payload: i64)
    linker.func_wrap(
        "elle",
        "rt_load_const",
        |caller: Caller<'_, ElleHost>, index: i32| -> (i64, i64) {
            let host = caller.data();
            let value = host.const_pool[index as usize];

            if value.tag < TAG_HEAP_START {
                (value.tag as i64, value.payload as i64)
            } else {
                // Heap value — look up handle. Constants were pre-inserted
                // into the handle table in create_store, in order.
                // Handle index = index + 1 (handle 0 is reserved).
                let handle = (index + 1) as u64;
                (value.tag as i64, handle as i64)
            }
        },
    )?;

    // rt_data_op(op: i32, args_ptr: i32, nargs: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "rt_data_op",
        |mut caller: Caller<'_, ElleHost>, op: i32, args_ptr: i32, nargs: i32| -> (i64, i64, i32) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let (bits, result) = dispatch_data_op(op, &args);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.0 as i32)
        },
    )?;

    Ok(linker)
}

/// Dispatch a data operation by opcode.
fn dispatch_data_op(op: i32, args: &[Value]) -> (crate::value::fiber::SignalBits, Value) {
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    use crate::value::heap::TableKey;

    let err = |kind: &str, msg: &str| (SIG_ERROR, crate::value::error_val(kind, msg));

    match op {
        0 => (SIG_OK, Value::cons(args[0], args[1])), // OP_CONS
        1 => match args[0].as_cons() {
            // OP_CAR
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        2 => match args[0].as_cons() {
            // OP_CDR
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::NIL),
        },
        3 => match args[0].as_cons() {
            // OP_CAR_DESTRUCTURE
            Some(c) => (SIG_OK, c.first),
            None => err("type-error", "car: not a pair"),
        },
        4 => match args[0].as_cons() {
            // OP_CDR_DESTRUCTURE
            Some(c) => (SIG_OK, c.rest),
            None => err("type-error", "cdr: not a pair"),
        },
        5 => match args[0].as_cons() {
            // OP_CAR_OR_NIL
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        6 => match args[0].as_cons() {
            // OP_CDR_OR_NIL
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::EMPTY_LIST),
        },
        7 => (SIG_OK, Value::array_mut(args.to_vec())), // OP_MAKE_ARRAY
        8 => (SIG_OK, Value::local_lbox(args[0])),      // OP_MAKE_LBOX
        9 => {
            // OP_LOAD_LBOX
            match args[0].as_lbox() {
                Some(cell) => (SIG_OK, *cell.borrow()),
                None => (SIG_OK, args[0]),
            }
        }
        10 => {
            // OP_STORE_LBOX
            if let Some(cell) = args[0].as_lbox() {
                *cell.borrow_mut() = args[1];
            }
            (SIG_OK, Value::NIL)
        }
        11 => (SIG_OK, Value::NIL), // OP_MAKE_STRING (stub)
        12 => {
            // OP_ARRAY_REF_DESTRUCTURE
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                if index < b.len() {
                    (SIG_OK, b[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else if let Some(arr) = args[0].as_array() {
                if index < arr.len() {
                    (SIG_OK, arr[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else {
                err("type-error", "array ref: not an array")
            }
        }
        13 => {
            // OP_ARRAY_SLICE_FROM
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, Value::array_mut(b[index.min(b.len())..].to_vec()))
            } else if let Some(arr) = args[0].as_array() {
                (
                    SIG_OK,
                    Value::array_mut(arr[index.min(arr.len())..].to_vec()),
                )
            } else {
                (SIG_OK, Value::array_mut(vec![]))
            }
        }
        14 => {
            // OP_STRUCT_GET_OR_NIL
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (SIG_OK, s.get(&key).copied().unwrap_or(Value::NIL))
            } else if let Some(s) = args[0].as_struct_mut() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (SIG_OK, s.borrow().get(&key).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        15 => {
            // OP_STRUCT_GET_DESTRUCTURE
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                match s.get(&key) {
                    Some(v) => (SIG_OK, *v),
                    None => err("key-error", "struct get: key not found"),
                }
            } else {
                err("type-error", "struct get: not a struct")
            }
        }
        16 => {
            // OP_ARRAY_EXTEND
            if let Some(arr) = args[0].as_array_mut() {
                if let Some(src) = args[1].as_array_mut() {
                    arr.borrow_mut().extend_from_slice(&src.borrow());
                } else if let Some(src) = args[1].as_array() {
                    arr.borrow_mut().extend_from_slice(src);
                }
            }
            (SIG_OK, args[0])
        }
        17 => {
            // OP_ARRAY_PUSH
            if let Some(arr) = args[0].as_array_mut() {
                arr.borrow_mut().push(args[1]);
            }
            (SIG_OK, args[0])
        }
        18 => {
            // OP_ARRAY_LEN
            let len = if let Some(arr) = args[0].as_array_mut() {
                arr.borrow().len()
            } else if let Some(arr) = args[0].as_array() {
                arr.len()
            } else {
                0
            };
            (SIG_OK, Value::int(len as i64))
        }
        19 => {
            // OP_ARRAY_REF_OR_NIL
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, b.get(index).copied().unwrap_or(Value::NIL))
            } else if let Some(arr) = args[0].as_array() {
                (SIG_OK, arr.get(index).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        _ => err("internal-error", &format!("rt_data_op: unknown op {op}")),
    }
}

/// Read args from linear memory as Vec<Value>.
fn read_args_from_memory(
    caller: &mut Caller<'_, ElleHost>,
    args_ptr: i32,
    nargs: i32,
) -> Vec<Value> {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory());

    let memory = match memory {
        Some(m) => m,
        None => return Vec::new(),
    };

    // First pass: read raw (tag, payload) pairs from memory
    let mut raw_pairs = Vec::with_capacity(nargs as usize);
    {
        let data = memory.data(&*caller);
        for i in 0..nargs as usize {
            let offset = args_ptr as usize + i * 16;
            let tag = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as u64;
            let payload =
                i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap()) as u64;
            raw_pairs.push((tag, payload));
        }
    }

    // Second pass: resolve heap handles
    let host = caller.data();
    raw_pairs
        .into_iter()
        .map(|(tag, payload)| {
            if tag < TAG_HEAP_START {
                Value { tag, payload }
            } else {
                host.handles.get(payload)
            }
        })
        .collect()
}

/// Compile WASM bytes into a Module.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<Module> {
    Module::new(engine, wasm_bytes)
}

/// Instantiate a module and call its entry function.
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
