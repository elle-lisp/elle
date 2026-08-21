//! Data-op dispatch and dynamic-parameter frame management:
//! `rt_data_op`, `rt_push_param`, `rt_pop_param`.

use wasmtime::*;

use crate::wasm::host::ElleHost;
use crate::wasm::linker::{dispatch_data_op, read_args_from_memory};

pub(super) fn register(linker: &mut Linker<ElleHost>) -> Result<()> {
    // rt_data_op(op: i32, args_ptr: i32, nargs: i32) -> (tag: i64, payload: i64, signal: i64)
    linker.func_wrap(
        "elle",
        "rt_data_op",
        |mut caller: Caller<'_, ElleHost>, op: i32, args_ptr: i32, nargs: i32| -> (i64, i64, i64) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let vm = caller.data().vm;
            let (bits, result) = dispatch_data_op(op, &args, vm);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.raw() as i64)
        },
    )?;

    // rt_push_param(args_ptr: i32, npairs: i32) -> ()
    linker.func_wrap(
        "elle",
        "rt_push_param",
        |mut caller: Caller<'_, ElleHost>, args_ptr: i32, npairs: i32| {
            let memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("rt_push_param: no memory");

            // Read (param, value) pairs from linear memory.
            // Each pair is 32 bytes: param(tag,payload) + value(tag,payload).
            let mut frame = Vec::with_capacity(npairs as usize);
            for i in 0..npairs as usize {
                let base = args_ptr as usize + i * 32;
                let data = memory.data(&caller);
                let read_i64 = |offset: usize| -> i64 {
                    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
                };
                let param_tag = read_i64(base) as u64;
                let param_payload = read_i64(base + 8) as u64;
                let val_tag = read_i64(base + 16) as u64;
                let val_payload = read_i64(base + 24) as u64;

                // Resolve param value from handle table
                let param_val = caller
                    .data()
                    .wasm_to_value(param_tag as i64, param_payload as i64);
                let value = caller
                    .data()
                    .wasm_to_value(val_tag as i64, val_payload as i64);

                // Extract parameter id
                if let Some((id, _)) = param_val.as_parameter() {
                    frame.push((id, value));
                }
            }
            caller.data_mut().param_frames.push(frame);
        },
    )?;

    // rt_pop_param() -> ()
    linker.func_wrap(
        "elle",
        "rt_pop_param",
        |mut caller: Caller<'_, ElleHost>| {
            caller.data_mut().param_frames.pop();
        },
    )?;

    Ok(())
}
