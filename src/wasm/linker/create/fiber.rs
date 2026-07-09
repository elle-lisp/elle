//! Fiber suspend/resume host functions:
//! `rt_yield`, `rt_get_resume_value`, `rt_load_saved_reg`.

use wasmtime::*;

use crate::wasm::host::ElleHost;
use crate::wasm::linker::read_reg_pairs;

pub(super) fn register(linker: &mut Linker<ElleHost>) -> Result<()> {
    // rt_yield(tag: i64, payload: i64, resume_state: i32, regs_ptr: i32, num_regs: i32, func_idx: i32, signal_bits: i64)
    // Save yielded value and live registers to a WasmSuspensionFrame.
    linker.func_wrap(
        "elle",
        "rt_yield",
        |mut caller: Caller<'_, ElleHost>,
         tag: i64,
         payload: i64,
         resume_state: i32,
         regs_ptr: i32,
         num_regs: i32,
         func_idx: i32,
         signal_bits: i64| {
            // Read saved registers from linear memory
            let saved_regs = read_reg_pairs(&mut caller, regs_ptr, num_regs);

            // Snapshot the executing closure so resume restores it (the store's self
            // slot is shared, so an interleaved fiber may overwrite it before resume).
            let self_memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("rt_yield: no memory");
            let (self_tag, self_payload) =
                crate::wasm::store::read_self_slot(&caller, &self_memory);

            if caller.data().debug {
                eprintln!(
                    "[rt_yield] tag={} payload={} resume_state={} num_regs={} func_idx={} signal_bits={}",
                    tag, payload, resume_state, num_regs, func_idx, signal_bits
                );
            }

            let host = caller.data_mut();
            host.push_suspension_frame(crate::wasm::host::WasmSuspensionFrame {
                wasm_func_idx: func_idx as u32,
                resume_state: resume_state as u32,
                saved_regs,
                env_snapshot: Vec::new(),
                env_base: 0,
                signal_bits: signal_bits as u64,
                self_tag,
                self_payload,
            });
        },
    )?;

    // rt_get_resume_value() -> (tag: i64, payload: i64)
    // Return the resume value set by the scheduler.
    linker.func_wrap(
        "elle",
        "rt_get_resume_value",
        |caller: Caller<'_, ElleHost>| -> (i64, i64) {
            let host = caller.data();
            let result = match host.resume_value {
                Some((tag, payload)) => (tag, payload),
                None => (crate::value::repr::TAG_NIL as i64, 0),
            };
            if caller.data().debug {
                eprintln!(
                    "[rt_get_resume_value] tag={} payload={} (resume_value={:?})",
                    result.0,
                    result.1,
                    host.resume_value.is_some()
                );
            }
            result
        },
    )?;

    // rt_load_saved_reg(index: i32) -> (tag: i64, payload: i64)
    // Load a saved register by index from the current suspension frame.
    linker.func_wrap(
        "elle",
        "rt_load_saved_reg",
        |caller: Caller<'_, ElleHost>, index: i32| -> (i64, i64) {
            let host = caller.data();
            // The front frame is always the one being resumed (innermost).
            // New frames pushed by rt_yield during the call go to the back.
            let frame_ref = host.first_suspension_frame();
            if let Some(frame) = frame_ref {
                if (index as usize) < frame.saved_regs.len() {
                    let (tag, pay) = frame.saved_regs[index as usize];
                    if caller.data().debug && index < 5 {
                        eprintln!(
                            "[rt_load_saved_reg] index={} tag={} payload={} (frame has {} regs)",
                            index,
                            tag,
                            pay,
                            frame.saved_regs.len()
                        );
                    }
                    (tag, pay)
                } else {
                    (crate::value::repr::TAG_NIL as i64, 0)
                }
            } else {
                if caller.data().debug {
                    eprintln!("[rt_load_saved_reg] NO FRAME! index={}", index);
                }
                (crate::value::repr::TAG_NIL as i64, 0)
            }
        },
    )?;

    Ok(())
}
