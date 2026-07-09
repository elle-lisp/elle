use super::*;

// The dispatch match and its longest inline opcode bodies live in submodules;
// each defines methods on `VM` that this harness calls. `use <mod>::*` is not
// needed — the moved items are inherent `impl VM` methods, resolved by the
// method-call syntax, not by name path.
mod opcodes;
mod params;
mod signals;

impl VM {
    /// Inner execution loop that handles all instructions.
    ///
    /// Takes `Rc` references to bytecode and constants so that yield and
    /// call handlers can capture them cheaply (Rc clone, not data copy).
    /// Derefs to slices for individual instruction handlers.
    ///
    /// Returns `(SignalBits, ip)` — the signal and the IP at exit.
    ///
    /// The per-instruction routing is `dispatch_instruction` (see `opcodes`);
    /// this body is just the harness around it: the pre-decode signal and
    /// alloc-limit checks, opcode decode, and the post-handler error check.
    /// (Branch/call fuel is charged inside `dispatch_instruction`.)
    pub(in crate::vm) fn execute_bytecode_inner_impl(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> (SignalBits, usize) {
        let mut ip = start_ip;
        let mut instr_ip = start_ip;

        // The template-derived context fields. Aliased here so the instruction
        // handlers below read the same names they always have; `code` bundles
        // them (see crate::value::Code).
        let bytecode: &Rc<Vec<u8>> = &code.bytecode;
        let constants: &Rc<Vec<Value>> = &code.constants;
        let location_map: &Rc<LocationMap> = &code.location_map;

        // Deref to slices for instruction handlers
        let bc: &[u8] = bytecode;
        let consts: &[Value] = constants;

        // The executing-closure register is a possibly-dead borrow here: an
        // activation can outlive its closure's heap value (the region solver
        // frees the value at its last use; `code`/`env` live on as `Rc`s), and a
        // parked frame restores the register long after that. So it must NOT be
        // dereferenced at dispatch entry. Its identity is verified where the
        // callee is live by construction — at the body-entry installs
        // (`debug_assert_entry_closure_matches`) — and `LoadSelf`, its reader,
        // asserts the register is populated (a self-recursive body's closure
        // region is kept live through the recursion by the tail-call adopt, so
        // the value LoadSelf reads is never stale).

        loop {
            // Check for pre-existing error signal (e.g., from previous Call)
            if let Some((bits, _)) = self.fiber.signal {
                if bits.contains(SIG_ERROR) || bits.contains(SIG_HALT) {
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    return (bits, ip);
                }
            }

            // Check for allocation limit violation from previous instruction.
            // The error flag is stored on the current FiberHeap (always installed
            // after chunk 1). Temporarily remove the limit so the error struct
            // can be allocated.
            if let Some((count, limit)) = self.heap().take_alloc_error() {
                let saved_limit = self.heap().set_object_limit(None);
                let err = self.escaping_error(
                    "allocation-error",
                    format!(
                        "heap object limit exceeded ({} objects, limit {})",
                        count, limit
                    ),
                );
                self.heap().set_object_limit(saved_limit);
                self.fiber.signal = Some((SIG_ERROR, err));
                if self.error_loc.is_none() {
                    self.error_loc = location_map.get(&instr_ip).cloned();
                }
                return (SIG_ERROR, ip);
            }

            if ip >= bc.len() {
                panic!("VM bug: Unexpected end of bytecode");
            }

            instr_ip = ip; // save instruction start before reading opcode
            let instr_byte = bc[ip];
            ip += 1;

            // Defined behavior for malformed bytecode: bytecode is produced
            // in-process by the compiler, so an invalid opcode byte means a
            // compiler bug or a corrupted buffer — panic with a message,
            // like the end-of-bytecode check above. (A bare transmute here
            // is UB for every byte value that is not a discriminant.) This
            // path must not allocate: no heap region is guaranteed to be
            // active when decoding fails.
            let Some(instr) = Instruction::from_byte(instr_byte) else {
                panic!(
                    "VM bug: invalid opcode 0x{:02x} at byte offset {}",
                    instr_byte, instr_ip
                );
            };

            // Fuel for the branch/call opcodes is charged inside
            // `dispatch_instruction` (via `charge_fuel`), on the same opcode
            // match that routes them — no separate pre-dispatch gate.
            if let Some(exit) = self.dispatch_instruction(
                instr,
                code,
                closure_env,
                bc,
                consts,
                constants,
                location_map,
                &mut ip,
                instr_ip,
            ) {
                return exit;
            }

            // Check for error signal set by this instruction's handler
            if let Some((bits, _)) = self.fiber.signal {
                if bits.contains(SIG_ERROR) || bits.contains(SIG_HALT) {
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    return (bits, ip);
                }
            }
        }
    }
}
