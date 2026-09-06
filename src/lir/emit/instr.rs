// audited: 2026-09-05
// docs/impl/bytecode.md
//! The emitter's instruction dispatch: what each `LirInstr` writes into the
//! bytecode, and what it does to the simulated operand stack.

use super::*;

impl Emitter {
    pub(super) fn emit_instr(&mut self, instr: &LirInstr, func: &LirFunction) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(value, func);
                self.push_reg(*dst);
            }

            LirInstr::ValueConst { dst, value } => {
                let const_idx = self.bytecode.add_constant(*value);
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(const_idx);
                self.push_reg(*dst);
            }

            LirInstr::MaterializeConst {
                dst,
                template,
                region,
            } => {
                // A heap literal (string, or quoted compound data) is an ordinary
                // allocation: emit the region slot, then the recursive template
                // inline in the (reclaimable) instruction stream — never a
                // pre-baked pool Value. The VM resolves the slot and
                // materializes a FRESH structure into that per-activation region.
                // A u32 byte-length prefix lets the disassembler skip the template
                // without decoding it.
                self.bytecode.emit(Instruction::MaterializeConst);
                self.bytecode.emit_u32(region.get());
                let mut buf = Vec::new();
                template.encode(&mut buf);
                self.bytecode.emit_u32(buf.len() as u32);
                for &b in &buf {
                    self.bytecode.emit_byte(b);
                }
                self.push_reg(*dst);
            }

            LirInstr::LoadLocal { dst, slot } => {
                self.bytecode.emit(Instruction::LoadLocal);
                self.bytecode.emit_u16(*slot);
                self.push_reg(*dst);
            }

            LirInstr::StoreLocal { slot, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StoreLocal);
                self.bytecode.emit_u16(*slot);
                // StoreLocal pops the value, stores it, and pushes it back.
                // Auto-pop: consume the pushed-back value so stores are pure
                // side effects from the LIR's perspective.
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::StoreLocalRefcounted { slot, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StoreLocal);
                self.bytecode.emit_u16(*slot);
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::LoadCapture { dst, index } => {
                if let Some(stack_slot) = Self::non_cell_local_slot(*index, func) {
                    // Non-cell locally-defined variable: use stack
                    self.bytecode.emit(Instruction::LoadLocal);
                    self.bytecode.emit_u16(stack_slot);
                } else {
                    self.bytecode.emit(Instruction::LoadUpvalue);
                    self.bytecode.emit_byte(0); // depth (currently unused)
                    self.bytecode.emit_u16(*index);
                }
                self.push_reg(*dst);
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Load without unwrapping cells - used for forwarding captures
                self.bytecode.emit(Instruction::LoadUpvalueRaw);
                self.bytecode.emit_byte(0); // depth (currently unused)
                self.bytecode.emit_u16(*index);
                self.push_reg(*dst);
            }

            LirInstr::LoadSelf { dst } => {
                // Pushes the executing closure from a runtime register — no
                // operand, no capture slot.
                self.bytecode.emit(Instruction::LoadSelf);
                self.push_reg(*dst);
            }

            LirInstr::StoreCapture { index, src } => {
                self.ensure_on_top(*src);
                if let Some(stack_slot) = Self::non_cell_local_slot(*index, func) {
                    // Non-cell locally-defined variable: use stack
                    self.bytecode.emit(Instruction::StoreLocal);
                    self.bytecode.emit_u16(stack_slot);
                } else {
                    self.bytecode.emit(Instruction::StoreUpvalue);
                    self.bytecode.emit_byte(0); // depth (currently unused)
                    self.bytecode.emit_u16(*index);
                }
                // Both StoreLocal and StoreUpvalue pop-then-push-back.
                // Auto-pop: consume the pushed-back value.
                self.bytecode.emit(Instruction::Pop);
                self.pop();
            }

            LirInstr::MakeClosure {
                dst,
                closure_id,
                captures,
                region,
            } => {
                // Check if captures are already in order on top of stack
                let stack_len = self.stack.len();
                let mut all_in_place = stack_len >= captures.len();
                if all_in_place {
                    let base = stack_len - captures.len();
                    for (i, cap) in captures.iter().enumerate() {
                        if self.reg_to_stack.get(cap) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                }

                if !all_in_place {
                    // Captures not in place - need to arrange them
                    for cap in captures {
                        self.ensure_on_top(*cap);
                    }
                }

                // Look up the pre-compiled closure by ClosureId.
                // In emit_module mode, closures are pre-compiled.
                // In standalone emit mode (tests), this panics — callers
                // must use emit_module for code with MakeClosure.
                let compiled = self
                    .compiled_closures
                    .as_ref()
                    .expect("MakeClosure without compiled_closures context")
                    .get(closure_id.0 as usize)
                    .expect("MakeClosure: invalid ClosureId")
                    .clone();

                // Look up the LirFunction from the module for metadata.
                // We need the LirFunction for the ClosureTemplate (arity,
                // signal, lbox masks, etc). The compiled_closures Vec is
                // parallel to the module's closures Vec.
                let func = &self
                    .closure_lir_funcs
                    .as_ref()
                    .expect("MakeClosure without closure_lir_funcs context")
                    [closure_id.0 as usize];

                // The nested lambda's TEMPLATE BLUEPRINT — plain compile-time
                // data, NOT a heap `Value` (a heap literal is an ordinary,
                // reclaimable allocation; closure templates are no exception).
                let template =
                    crate::value::TemplateProto::nested_lambda(func, captures.len(), compiled);

                // Register the blueprint in THIS code object's child_protos and
                // emit its index; the VM/JIT materialize a fresh region-allocated
                // template from it per execution.
                let proto_idx = self.bytecode.child_protos.len() as u16;
                self.bytecode.child_protos.push(Rc::new(template));

                // Emit MakeClosure instruction (region operand emitted first so the region is in place before the alloc)
                self.bytecode.emit(Instruction::MakeClosure);
                self.bytecode.emit_u32(region.get());
                self.bytecode.emit_u16(proto_idx);
                self.bytecode.emit_u16(captures.len() as u16);

                // Pop captures, push closure
                for _ in captures {
                    self.pop();
                }
                self.push_reg(*dst);
            }

            LirInstr::Call {
                dst,
                func,
                args,
                arity_checked,
                region,
            }
            | LirInstr::SuspendingCall {
                dst,
                func,
                args,
                arity_checked,
                region,
            } => {
                // Call expects: [arg1, arg2, ..., argN, func] on stack
                // Check if values are already in the correct positions at the top of the stack
                let total_values = args.len() + 1; // args + func
                let stack_len = self.stack.len();

                // Check if all values are already in place
                let mut all_in_place = stack_len >= total_values;
                if all_in_place {
                    let base = stack_len - total_values;
                    for (i, arg) in args.iter().enumerate() {
                        if self.reg_to_stack.get(arg) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                    if all_in_place && self.reg_to_stack.get(func) != Some(&(base + args.len())) {
                        all_in_place = false;
                    }
                }

                if !all_in_place {
                    // Values are not in place, need to duplicate them to the top
                    for arg in args {
                        self.ensure_on_top(*arg);
                    }
                    self.ensure_on_top(*func);
                }

                if *arity_checked {
                    self.bytecode.emit(Instruction::CallChecked);
                } else {
                    self.bytecode.emit(Instruction::Call);
                }
                self.bytecode.emit_u16(args.len() as u16);
                self.bytecode.emit_u32(region.get());
                let call_resume_ip = self.bytecode.current_pos();

                // Pop func and args from simulated stack
                self.pop(); // func
                for _ in args {
                    self.pop();
                }

                // Record call site metadata AFTER popping func/args, BEFORE
                // pushing result. This matches the interpreter's stack state
                // when yield propagates through a call: the Call instruction
                // has consumed its operands, the callee yielded, and the
                // interpreter saves the remaining stack.
                if self.current_func_may_suspend {
                    self.call_sites.push(CallSiteInfo {
                        resume_ip: call_resume_ip,
                        stack_regs: self.stack.clone(),
                        num_locals: self.current_func_num_locals,
                    });
                }

                self.push_reg(*dst);
            }

            LirInstr::TailCall {
                func,
                args,
                arity_checked,
                region,
                defer_callee_release,
                deferred_release_slot,
                borrowed_arg_slots,
                // The stack-based VM leaves a normally-completing native's
                // result on the operand stack for `Return` to pop; `dst` is a
                // JIT-only binding (see `LirInstr::TailCall`).
                dst: _,
            } => {
                // Check if values are already in the correct positions at the top of the stack
                let total_values = args.len() + 1; // args + func
                let stack_len = self.stack.len();

                let mut all_in_place = stack_len >= total_values;
                if all_in_place {
                    let base = stack_len - total_values;
                    for (i, arg) in args.iter().enumerate() {
                        if self.reg_to_stack.get(arg) != Some(&(base + i)) {
                            all_in_place = false;
                            break;
                        }
                    }
                    if all_in_place && self.reg_to_stack.get(func) != Some(&(base + args.len())) {
                        all_in_place = false;
                    }
                }

                if !all_in_place {
                    for arg in args {
                        self.ensure_on_top(*arg);
                    }
                    self.ensure_on_top(*func);
                }
                if *arity_checked {
                    self.bytecode.emit(Instruction::TailCallChecked);
                } else {
                    self.bytecode.emit(Instruction::TailCall);
                }
                self.bytecode.emit_u16(args.len() as u16);
                self.bytecode.emit_u32(region.get());
                // Adopt-callee flag: 1 ⇒ the runtime releases the callee closure's
                // region when the new activation completes (the dead-past-TailCall
                // decref). See `LirInstr::TailCall::defer_callee_release`.
                self.bytecode
                    .emit_byte(if *defer_callee_release { 1 } else { 0 });
                // Closure-cycle merged-arena adopt slot (u32; `0` = None, since a
                // `StaticRegion` is `NonZeroU32`). When the callee resolves to a
                // closure at runtime the new activation adopts THIS slot's region;
                // a native callee never consumes it (the live scope-exit
                // `DecrefRegion` frees the arena). See
                // `LirInstr::TailCall::deferred_release_slot`.
                self.bytecode
                    .emit_u32(deferred_release_slot.map_or(0, |s| s.get()));
                // The borrowed-argument stash slots, so a signal exit can
                // consume the retains the fall-through block below would have
                // (docs/impl/region/mechanism.md § "What the fall-through owes,
                // a signal exit owes too"). A count byte then one u16 slot each,
                // so a call with no borrowed argument — the overwhelming
                // majority — costs exactly the zero byte. Truncated at 255: a
                // longer argument list than any real call has, whose tail keeps
                // the over-keep the abandoned block always had.
                let n = borrowed_arg_slots.len().min(u8::MAX as usize);
                self.bytecode.emit_byte(n as u8);
                for &slot in &borrowed_arg_slots[..n] {
                    self.bytecode.emit_u16(slot);
                }
            }

            LirInstr::List {
                dst,
                head,
                tail,
                region,
            } => {
                // VM pops rest (top) then first (below), calls pair(first, rest).
                // Push head first (it becomes below = first), then tail (top = rest).
                self.ensure_on_top(*head);
                self.ensure_on_top(*tail);
                self.bytecode.emit(Instruction::Pair);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::MakeArrayMut {
                dst,
                elements,
                region,
            } => {
                for elem in elements {
                    self.ensure_on_top(*elem);
                }
                self.bytecode.emit(Instruction::MakeArrayMut);
                self.bytecode.emit_u32(region.get());
                self.bytecode.emit_byte(elements.len() as u8);
                for _ in elements {
                    self.pop();
                }
                self.push_reg(*dst);
            }

            _ => self.emit_instr_destructure(instr),
        }
    }
}

mod destructure;
mod ops;
