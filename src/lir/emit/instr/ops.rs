// audited: 2026-09-06
// src/lir/AGENTS.md
//! Emitting the operator, predicate, region-refcount and collection-op
//! instructions to bytecode.
//!
//! One exhaustive match over the instruction set, which is why this file takes
//! the dispatch-table allowance in `src/lir/AGENTS.md`. Each arm brings its
//! operands to the top of the simulated stack, emits an opcode, and records the
//! stack effect.

use super::*;

impl Emitter {
    /// Operator, predicate, region-refcount, and collection-op instruction
    /// emission (chain tail from `emit_instr`).
    pub(super) fn emit_instr_ops(&mut self, instr: &LirInstr) {
        match instr {
            LirInstr::BinOp {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                // Check if lhs and rhs are already the top two stack elements
                // (lhs at top-1, rhs at top). This is the common case from the
                // lowerer and avoids DupN which would leave orphaned values.
                self.ensure_binary_on_top(*lhs, *rhs);
                // Four operations have an integer-only opcode, which reads both
                // operands without testing a tag. The rest have one opcode each:
                // `Rem` was never specialized, and the bitwise handlers already
                // read integers (docs/impl/lir.md).
                let int_proven = proof.is_int();
                let instr = match op {
                    BinOp::Add if int_proven => Instruction::AddInt,
                    BinOp::Sub if int_proven => Instruction::SubInt,
                    BinOp::Mul if int_proven => Instruction::MulInt,
                    BinOp::Div if int_proven => Instruction::DivInt,
                    BinOp::Add => Instruction::Add,
                    BinOp::Sub => Instruction::Sub,
                    BinOp::Mul => Instruction::Mul,
                    BinOp::Div => Instruction::Div,
                    BinOp::Rem => Instruction::Rem,
                    BinOp::BitAnd => Instruction::BitAnd,
                    BinOp::BitOr => Instruction::BitOr,
                    BinOp::BitXor => Instruction::BitXor,
                    BinOp::Shl => Instruction::Shl,
                    BinOp::Shr => Instruction::Shr,
                };
                self.bytecode.emit(instr);
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Compare {
                dst,
                op,
                lhs,
                rhs,
                proof: _,
            } => {
                // Check if lhs and rhs are already the top two stack elements
                self.ensure_binary_on_top(*lhs, *rhs);
                let instr = match op {
                    CmpOp::Eq => Instruction::Eq,
                    CmpOp::Lt => Instruction::Lt,
                    CmpOp::Gt => Instruction::Gt,
                    CmpOp::Le => Instruction::Le,
                    CmpOp::Ge => Instruction::Ge,
                    CmpOp::Ne => Instruction::Eq, // Will need Not after
                };
                self.bytecode.emit(instr);
                if matches!(op, CmpOp::Ne) {
                    self.bytecode.emit(Instruction::Not);
                }
                self.pop();
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::UnaryOp {
                dst,
                op,
                src,
                proof: _,
            } => {
                self.ensure_on_top(*src);
                match op {
                    UnaryOp::Not => self.bytecode.emit(Instruction::Not),
                    UnaryOp::Neg => {
                        // Negate by multiplying by -1.
                        // Stack has src on top; push -1, then Mul.
                        let neg1_idx = self.bytecode.add_constant(Value::int(-1));
                        self.bytecode.emit(Instruction::LoadConst);
                        self.bytecode.emit_u16(neg1_idx);
                        self.bytecode.emit(Instruction::Mul);
                    }
                    UnaryOp::BitNot => {
                        self.bytecode.emit(Instruction::BitNot);
                    }
                }
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Convert { dst, op, src } => {
                self.ensure_on_top(*src);
                match op {
                    ConvOp::IntToFloat => self.bytecode.emit(Instruction::IntToFloat),
                    ConvOp::FloatToInt => self.bytecode.emit(Instruction::FloatToInt),
                }
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsPair { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsPair);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsArray { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsArray);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsArrayMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsArrayMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsStruct { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsStruct);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsStructMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsStructMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsSet { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSet);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::IsSetMut { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSetMut);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutLen { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutLen);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::MakeCaptureCell { dst, value, region } => {
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::MakeCapture);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::LoadCaptureCell { dst, cell } => {
                self.ensure_on_top(*cell);
                self.bytecode.emit(Instruction::UnwrapCapture);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StoreCaptureCell { cell, value } => {
                self.ensure_on_top(*cell);
                self.ensure_on_top(*value);
                self.bytecode.emit(Instruction::UpdateCapture);
                // UpdateCapture pops value, pops cell, pushes value back.
                // Unlike other stores, UpdateCapture pushes the value back.
                // We do NOT auto-pop here because lower_set needs the value.
                self.pop(); // value (consumed by UpdateCapture, re-pushed)
                self.pop(); // cell (consumed by UpdateCapture)
                            // Value is now on the stack (pushed back by UpdateCapture).
                self.push_reg(*value);
            }

            LirInstr::LoadResumeValue { dst } => {
                // The resume value is already on the operand stack
                // (pushed by the VM's resume_continuation).
                // The stack simulation already has the pre-yield state.
                // Just register the resume value.
                self.push_reg(*dst);
            }

            LirInstr::Eval { dst, expr, env } => {
                // Stack order: env on bottom, expr on top
                // (VM pops expr first, then env)
                self.ensure_on_top(*env);
                self.ensure_on_top(*expr);
                self.bytecode.emit(Instruction::Eval);
                // Eval pops 2 values and pushes 1 result
                self.pop(); // expr
                self.pop(); // env
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutExtend { dst, array, source } => {
                // Stack: [array, source] → [extended_array]
                self.ensure_binary_on_top(*array, *source);
                self.bytecode.emit(Instruction::ArrayMutExtend);
                self.pop(); // source
                self.pop(); // array
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutPush { dst, array, value } => {
                // Stack: [array, value] → [extended_array]
                self.ensure_binary_on_top(*array, *value);
                self.bytecode.emit(Instruction::ArrayMutPush);
                self.pop(); // value
                self.pop(); // array
                self.push_reg(*dst);
            }

            LirInstr::CallArrayMut {
                dst,
                func,
                args,
                region,
                args_region,
            } => {
                // Stack: [func, args_array] → [result]
                self.ensure_binary_on_top(*func, *args);
                self.bytecode.emit(Instruction::CallArrayMut);
                self.bytecode.emit_u32(region.get());
                self.bytecode.emit_u32(args_region.get());
                let call_resume_ip = self.bytecode.current_pos();
                self.pop(); // args
                self.pop(); // func

                if self.current_func_may_suspend {
                    self.call_sites.push(CallSiteInfo {
                        resume_ip: call_resume_ip,
                        stack_regs: self.stack.clone(),
                        num_locals: self.current_func_num_locals,
                    });
                }

                self.push_reg(*dst);
            }

            LirInstr::TailCallArrayMut {
                func,
                args,
                region,
                args_region,
            } => {
                // Stack: [func, args_array] → (tail call, no push)
                self.ensure_binary_on_top(*func, *args);
                self.bytecode.emit(Instruction::TailCallArrayMut);
                self.bytecode.emit_u32(region.get());
                self.bytecode.emit_u32(args_region.get());
                self.pop(); // args
                self.pop(); // func
            }

            LirInstr::IncrefRegion { region_id } => {
                self.bytecode.emit(Instruction::IncrefRegion);
                self.bytecode.emit_u32(region_id.get());
            }

            LirInstr::DecrefValueRegion { src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::DecrefValueRegion);
                // Value popped by the release handler.
                self.pop();
            }

            LirInstr::DecrefCellRegion { src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::DecrefCellRegion);
                // Value popped by the release handler (frees the cell's own
                // region via `region_of`, not the unwrapped inner value).
                self.pop();
            }

            LirInstr::IncrefValueRegion { src } => {
                // Unlike DecrefValueRegion, retain does NOT consume the
                // value: it is the function's result and must remain on
                // top for the caller. The handler peeks rather than pops,
                // so the stack model is unchanged (`src` stays live).
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IncrefValueRegion);
            }

            LirInstr::AdoptRegion { parent, child } => {
                // Value-resolved adopt (the ownership forest): bring both values
                // to the top — parent at top-1, child at top — emit the op, and
                // consume both. The handler reads each value's runtime region and
                // links the child's into the parent's Owned subtree.
                self.ensure_binary_on_top(*parent, *child);
                self.bytecode.emit(Instruction::AdoptRegion);
                self.pop(); // child
                self.pop(); // parent
            }

            LirInstr::AdoptCellRegion { parent, child } => {
                // Value-resolved cell adopt (the ownership forest): identical stack
                // shape to `AdoptRegion`, but the handler resolves both operands with
                // `region_of` (NOT `result_region_of`), so a `CaptureCell` child's OWN
                // region is adopted — the cell↔closure containment
                // (docs/impl/region/adopt.md § "The capture adopt").
                self.ensure_binary_on_top(*parent, *child);
                self.bytecode.emit(Instruction::AdoptCellRegion);
                self.pop(); // child
                self.pop(); // parent
            }

            LirInstr::AdoptIntoActivation { child } => {
                // Value-resolved activation adopt (the ownership forest's owner
                // node): bring the child value to the top, emit the op, and
                // consume it. The handler resolves the child's runtime region
                // and links it into the current activation's lazily-minted
                // owner node (docs/impl/region/owner.md § "Owner nodes").
                self.ensure_on_top(*child);
                self.bytecode.emit(Instruction::AdoptIntoActivation);
                self.pop(); // child
            }

            LirInstr::FreeRegionGroup { members } => {
                // Value-resolved co-owned group free (the ownership forest): bring every
                // member value to the top, emit the op with the member count, and consume
                // them all. The handler resolves each value's runtime region and frees the
                // whole set as one wholesale subtree drop. Produces no value (it replaces
                // the members' individual decrefs).
                for m in members {
                    self.ensure_on_top(*m);
                }
                self.bytecode.emit(Instruction::FreeRegionGroup);
                self.bytecode.emit_byte(members.len() as u8);
                for _ in members {
                    self.pop();
                }
            }

            LirInstr::AssertRegionMatches { region_id, src } => {
                // The coalescing oracle peeks the value the coalesced slot is
                // claimed to name (it must stay on top — the subsequent
                // `IncrefRegion`/`Return` reads it), then carries the slot as a
                // u32 operand, exactly like `IncrefRegion`. Stack-neutral.
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::AssertRegionMatches);
                self.bytecode.emit_u32(region_id.get());
            }

            LirInstr::DecrefRegion { region_id } => {
                self.bytecode.emit(Instruction::DecrefRegion);
                self.bytecode.emit_u32(region_id.get());
            }

            LirInstr::PushParamFrame { pairs } => {
                // Push all param/value pairs onto the stack
                for (param, value) in pairs {
                    self.ensure_on_top(*param);
                    self.ensure_on_top(*value);
                }
                self.bytecode.emit(Instruction::PushParamFrame);
                self.bytecode.emit_byte(pairs.len() as u8);
                // All pairs consumed from stack
                for _ in pairs {
                    self.pop(); // value
                    self.pop(); // param
                }
            }

            LirInstr::PopParamFrame => {
                self.bytecode.emit(Instruction::PopParamFrame);
                // No stack effect
            }

            // === New type predicates ===
            LirInstr::IsEmpty { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsEmptyList);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBool { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBool);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsInt { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsInt);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFloat { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFloat);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsString { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsString);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsKeyword { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsKeyword);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsSymbolCheck { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsSymbol);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBytes { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBytes);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsBox { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsBox);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsClosure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsClosure);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::IsFiber { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IsFiber);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::TypeOf { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::TypeOf);
                self.pop();
                self.push_reg(*dst);
            }

            // === Data access ===
            LirInstr::Length { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::Length);
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Get { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrGet);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Put { dst, obj, key, val } => {
                self.ensure_on_top(*obj);
                self.ensure_on_top(*key);
                self.ensure_on_top(*val);
                self.bytecode.emit(Instruction::IntrPut);
                self.pop(); // val
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Del { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrDel);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::Has { dst, obj, key } => {
                self.ensure_binary_on_top(*obj, *key);
                self.bytecode.emit(Instruction::IntrHas);
                self.pop(); // key
                self.pop(); // obj
                self.push_reg(*dst);
            }
            LirInstr::IntrPush { dst, array, value } => {
                self.ensure_binary_on_top(*array, *value);
                self.bytecode.emit(Instruction::IntrPush);
                self.pop(); // value
                self.pop(); // array
                self.push_reg(*dst);
            }
            LirInstr::IntrStringPush { dst, string, value } => {
                self.ensure_binary_on_top(*string, *value);
                self.bytecode.emit(Instruction::IntrStringPush);
                self.pop(); // value
                self.pop(); // string
                self.push_reg(*dst);
            }
            LirInstr::IntrBytesPush { dst, bytes, value } => {
                self.ensure_binary_on_top(*bytes, *value);
                self.bytecode.emit(Instruction::IntrBytesPush);
                self.pop(); // value
                self.pop(); // bytes
                self.push_reg(*dst);
            }
            LirInstr::Pop { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrPop);
                self.pop();
                self.push_reg(*dst);
            }

            // === Mutability ===
            LirInstr::Freeze { dst, src, region } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrFreeze);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.push_reg(*dst);
            }
            LirInstr::Thaw { dst, src, region } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::IntrThaw);
                self.bytecode.emit_u32(region.get());
                self.pop();
                self.push_reg(*dst);
            }

            // === Identity ===
            LirInstr::Identical { dst, lhs, rhs } => {
                self.ensure_binary_on_top(*lhs, *rhs);
                self.bytecode.emit(Instruction::Identical);
                self.pop(); // rhs
                self.pop(); // lhs
                self.push_reg(*dst);
            }

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::CheckSignalBound);
                self.bytecode.emit_signal_bits(*allowed_bits);
                // Value consumed by the check
                self.pop();
            }
            _ => unreachable!("emit_instr_ops: instruction handled in emit_instr"),
        }
    }
}
