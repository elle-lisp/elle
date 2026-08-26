use super::*;

// The dispatch match and its longest inline opcode bodies live in submodules;
// each defines methods on `VM` that this harness calls. `use <mod>::*` is not
// needed — the moved items are inherent `impl VM` methods, resolved by the
// method-call syntax, not by name path.
mod opcodes;
mod params;
mod signals;

impl VM {
    /// Debug-only: confirm the frame's reserved local region is still intact
    /// before executing the instruction at `instr_ip`.
    ///
    /// The emitter opens each body by pushing one `Nil` per local, so local `n`
    /// occupies stack position `frame_base + n` and operands stack above them
    /// (`Code::reserved_locals`). Nothing in a well-formed body pops through
    /// that floor. When something does, the frame silently loses its top
    /// local(s): stores meant for a local land on an operand, reads return a
    /// neighbour's value, and the failure only becomes visible much later — when
    /// a `LoadLocal` of a high slot finally indexes past the end of the stack,
    /// often thousands of instructions and several suspensions away from the
    /// code that did it. Checking the floor per instruction names the culprit
    /// instead, with its source location.
    /// The prologue itself is exempt: it is the `reserved_locals` single-byte
    /// `Nil` opcodes at offsets `0..reserved_locals`, and it is what establishes
    /// the floor, so the region is only guaranteed complete past it.
    #[cfg(debug_assertions)]
    fn debug_assert_locals_intact(&self, code: &crate::value::Code, instr_ip: usize) {
        if instr_ip < code.reserved_locals {
            return;
        }
        let floor = self.current_frame_base() + code.reserved_locals;
        if self.fiber.stack.len() >= floor {
            return;
        }
        let loc = code
            .location_map
            .get(&instr_ip)
            .map(|l| format!("{l}"))
            .unwrap_or_else(|| "<no source location>".to_string());
        panic!(
            "VM bug: the frame's reserved local region has been popped into at ip \
             {instr_ip} ({loc}): the stack holds {} value(s) but this body reserves \
             {} local slot(s) above frame base {}. A local slot no longer exists, so \
             later local reads and writes at this depth address the wrong values \
             (src/vm/dispatch/interp.rs, `debug_assert_locals_intact`)",
            self.fiber.stack.len(),
            code.reserved_locals,
            self.current_frame_base(),
        );
    }

    /// Record the source location of the instruction at `instr_ip` as the
    /// origin of the error now leaving this frame.
    ///
    /// First-writer-wins: the frame that raised reaches its exit path before
    /// any frame it unwinds through, so the innermost location is the one that
    /// reaches the root (docs/impl/vm.md § "Where a reported error's location
    /// comes from"). `VM::absorbs` takes the record when a mask catches the
    /// error, so the slot an outer frame finds full always belongs to the
    /// error it is carrying.
    fn record_error_loc(&mut self, location_map: &LocationMap, instr_ip: usize) {
        if self.error_loc.is_none() {
            self.error_loc = location_map.get(&instr_ip).cloned();
        }
    }

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
        // region is kept live through the recursion by the tail-call deferred release, so
        // the value LoadSelf reads is never stale).

        loop {
            // Check for pre-existing error signal (e.g., from previous Call)
            if let Some((bits, _)) = self.fiber.signal {
                if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) {
                    self.record_error_loc(location_map, instr_ip);
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
                self.record_error_loc(location_map, instr_ip);
                return (SIG_ERROR, ip);
            }

            if ip >= bc.len() {
                panic!("VM bug: Unexpected end of bytecode");
            }

            instr_ip = ip; // save instruction start before reading opcode

            // Locals live beneath the operands on this same stack, so nothing
            // may pop through the reserved region (see `Code::reserved_locals`).
            #[cfg(debug_assertions)]
            self.debug_assert_locals_intact(code, instr_ip);

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
                // The dominant error exit: a handler that raises (or that
                // propagates a callee's raise) returns the signal here rather
                // than falling through to the post-handler check below, so this
                // is where most errors get the location of the form that
                // raised them.
                let (exit_bits, _) = exit;
                if exit_bits.intersects(SIG_ERROR) || exit_bits.intersects(SIG_HALT) {
                    self.record_error_loc(location_map, instr_ip);
                }
                return exit;
            }

            // Check for error signal set by this instruction's handler
            if let Some((bits, _)) = self.fiber.signal {
                if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) {
                    self.record_error_loc(location_map, instr_ip);
                    return (bits, ip);
                }
            }
        }
    }
}
