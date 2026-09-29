// audited: 2026-09-28
//! The interpreter's inner loop: decode one opcode, route it, and check what
//! the handler left on the fiber.
//!
//! docs/impl/vm.md

use super::*;

// The dispatch match and its longest inline opcode bodies live in submodules;
// each defines methods on `VM` that this harness calls. `use <mod>::*` is not
// needed — the moved items are inherent `impl VM` methods, resolved by the
// method-call syntax, not by name path.
mod opcodes;
mod params;
mod scalar;
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
        if instr_ip < code.reserved_locals() {
            return;
        }
        let floor = self.current_frame_base() + code.reserved_locals();
        if self.fiber.stack.len() >= floor {
            return;
        }
        let loc = code
            .locations()
            .get(instr_ip)
            .map(|l| format!("{l}"))
            .unwrap_or_else(|| "<no source location>".to_string());
        panic!(
            "VM bug: the frame's reserved local region has been popped into at ip \
             {instr_ip} ({loc}): the stack holds {} value(s) but this body reserves \
             {} local slot(s) above frame base {}. A local slot no longer exists, so \
             later local reads and writes at this depth address the wrong values \
             (src/vm/dispatch/interp.rs, `debug_assert_locals_intact`)",
            self.fiber.stack.len(),
            code.reserved_locals(),
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
    pub(in crate::vm) fn record_error_loc(
        &mut self,
        locations: crate::value::closure::LocationTable<'_>,
        instr_ip: usize,
    ) {
        if self.error_loc.is_none() {
            self.error_loc = locations.get(instr_ip);
        }
    }

    /// Take the placeholder a raising instruction left in its result position,
    /// and answer the raise's site (docs/impl/vm.md § "The error exit").
    ///
    /// An instruction with a result pushes one placeholder in place of it when
    /// it raises, and the pop leaves that position empty for a restart's value.
    /// `CheckSignalBound` and `PushParamFrame` produce no value, so they push
    /// none, and a restart delivers nothing to them. Popping for one of those
    /// would take a live operand instead, and the debug check names that case: a
    /// placeholder is `nil` (or the empty list a rest-destructure leaves), and
    /// it sits above the frame's locals.
    fn take_raise_placeholder(
        &mut self,
        instr: Instruction,
        code: &crate::value::Code,
    ) -> crate::value::fiber::RaiseSite {
        use crate::value::fiber::RaiseSite;
        if matches!(
            instr,
            Instruction::CheckSignalBound | Instruction::PushParamFrame
        ) {
            return RaiseSite::NoResult;
        }
        #[cfg(debug_assertions)]
        {
            let floor = self.current_frame_base() + code.reserved_locals();
            debug_assert!(
                self.fiber.stack.len() > floor,
                "VM bug: {instr:?} raised with no placeholder above the frame's \
                 {} local slot(s)",
                code.reserved_locals(),
            );
        }
        #[cfg(not(debug_assertions))]
        let _ = code;
        let placeholder = self.fiber.stack.pop();
        debug_assert!(
            placeholder.is_some_and(|v| v.is_nil() || v == Value::EMPTY_LIST),
            "VM bug: {instr:?} raised with {placeholder:?} in its result position \
             where a placeholder belongs",
        );
        RaiseSite::Call
    }

    /// Take the result of the instruction whose allocation the object limit
    /// refused (docs/impl/vm.md § "The error exit"). Every instruction that
    /// allocates pushes one result, and the refusal leaves it as no value — a
    /// `nil` where the heap refused the object, or a structure holding one —
    /// so a restart answers the instruction in its place.
    fn take_refused_result(&mut self, code: &crate::value::Code) {
        #[cfg(debug_assertions)]
        {
            let floor = self.current_frame_base() + code.reserved_locals();
            debug_assert!(
                self.fiber.stack.len() > floor,
                "VM bug: the object limit refused an allocation, but no result sits \
                 above the frame's {} local slot(s)",
                code.reserved_locals(),
            );
        }
        #[cfg(not(debug_assertions))]
        let _ = code;
        self.fiber.stack.pop();
    }

    /// Inner execution loop that handles all instructions.
    ///
    /// Takes `Rc` references to bytecode and constants so that yield and
    /// call handlers can capture them cheaply (Rc clone, not data copy).
    /// Derefs to slices for individual instruction handlers.
    ///
    /// Returns the `Exit`: the signal, the IP at exit, and where an error was
    /// raised.
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
    ) -> crate::vm::execute::Exit {
        use crate::value::fiber::RaiseSite;
        use crate::vm::execute::Exit;
        let mut ip = start_ip;
        let mut instr_ip = start_ip;

        // The template-derived context fields. Aliased here so the instruction
        // handlers below read the same names they always have; `code` bundles
        // them (see crate::value::Code).
        let bc: &[u8] = code.bytecode();
        let consts: &[Value] = code.constants();
        let locations = code.locations();

        // `--trace=arena` attribution (docs/impl/region/diagnostics.md): the bit
        // is read ONCE per frame, so an untraced run pays a predictable branch
        // on a local per instruction and resolves no location. A toggle through
        // `(vm/config-set :trace …)` therefore takes effect at the next frame,
        // which is what a diagnostic needs and what keeps this off the hot path.
        let trace_arena = self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::ARENA);
        let arena_fn_name = if trace_arena {
            code.template().name()
        } else {
            None
        };

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

        // Whether an instruction of this frame has run, which decides whose
        // allocation a refusal by the object limit was (below).
        let mut ran_instruction = false;
        loop {
            // Check for pre-existing error signal (e.g., from previous Call)
            if let Some((bits, _)) = self.fiber.signal {
                if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) {
                    self.record_error_loc(locations, instr_ip);
                    return Exit::at(bits, ip);
                }
            }

            // Check for an allocation the object limit refused since the last
            // check. The error flag is stored on the heap. Temporarily remove
            // the limit so the error struct can be allocated. A refusal by this
            // frame's previous instruction left that instruction's result as no
            // value, so the raise is that instruction's, and a restart answers
            // it. A refusal before this frame ran anything — a callee's
            // environment, built before its first instruction — has no result
            // position here (docs/impl/vm.md § "The error exit").
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
                self.record_error_loc(locations, instr_ip);
                let site = if ran_instruction {
                    self.take_refused_result(code);
                    RaiseSite::Call
                } else {
                    RaiseSite::NoResult
                };
                return Exit::raised(SIG_ERROR, ip, site);
            }

            if ip >= bc.len() {
                panic!("VM bug: Unexpected end of bytecode");
            }

            instr_ip = ip; // save instruction start before reading opcode

            if trace_arena {
                self.arena_site = locations.get(instr_ip).map(|l| (l, arena_fn_name));
            }

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
                locations,
                &mut ip,
                instr_ip,
            ) {
                // The exit a handler returns directly: an `Emit`, a tail or
                // spliced call's raise, a suspend. A handler that returns here
                // leaves nothing in the result position, so there is nothing
                // to pop.
                let (exit_bits, exit_ip) = exit;
                if exit_bits.intersects(SIG_ERROR) || exit_bits.intersects(SIG_HALT) {
                    self.record_error_loc(locations, instr_ip);
                }
                let site = if instr == Instruction::Emit {
                    RaiseSite::Emit
                } else {
                    RaiseSite::Call
                };
                return Exit::raised(exit_bits, exit_ip, site);
            }

            // The dominant error exit: a handler that raises sets the signal,
            // pushes its placeholder, and falls through to here, so this is
            // where most errors get the location of the form that raised them.
            if let Some((bits, _)) = self.fiber.signal {
                if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) {
                    self.record_error_loc(locations, instr_ip);
                    let site = if bits.intersects(SIG_ERROR) {
                        self.take_raise_placeholder(instr, code)
                    } else {
                        RaiseSite::Call
                    };
                    return Exit::raised(bits, ip, site);
                }
            }

            // The instruction ran to completion in this frame, so an
            // allocation the object limit refuses from here on is its.
            ran_instruction = true;
        }
    }
}
