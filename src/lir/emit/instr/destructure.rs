// audited: 2026-09-05
// docs/impl/bytecode.md
//! Reads that take a value apart: a pair's halves, an array element, a struct
//! field, and the silent forms a parameter pattern uses.

use super::*;

/// The constant a struct or table key lowers to.
///
/// The pattern and access-path lowerers build only keyword and symbol keys, so
/// a string one never reaches the emitter — a constant pool has no region, and
/// a string is a heap value that would need one.
fn struct_key(key: &LirConst, site: &str) -> Value {
    match key {
        LirConst::Keyword(hash) => Value::keyword_from_hash(*hash),
        LirConst::String(_) => {
            unreachable!("struct keys are keyword or symbol, never string")
        }
        LirConst::Int(n) => Value::int(*n),
        LirConst::Symbol(sym) => Value::symbol(*sym),
        LirConst::Bool(b) => Value::bool(*b),
        LirConst::Nil => Value::NIL,
        _ => panic!("{site}: unsupported key type"),
    }
}

impl Emitter {
    /// The destructuring reads (chain link from `emit_instr`).
    pub(super) fn emit_instr_destructure(&mut self, instr: &LirInstr) {
        match instr {
            LirInstr::First { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::First);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::Rest { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::Rest);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::MatchFail { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::MatchFail);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::FirstDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::RestDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutRefDestructure);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutSliceFrom);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructGetOrNil { dst, src, key } => {
                self.ensure_on_top(*src);
                let const_idx = self
                    .bytecode
                    .add_constant(struct_key(key, "StructGetOrNil"));
                self.bytecode.emit(Instruction::StructGetOrNil);
                self.bytecode.emit_u16(const_idx);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructGetDestructure { dst, src, key } => {
                self.ensure_on_top(*src);
                let const_idx = self
                    .bytecode
                    .add_constant(struct_key(key, "StructGetDestructure"));
                self.bytecode.emit(Instruction::StructGetDestructure);
                self.bytecode.emit_u16(const_idx);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StructRest);
                self.bytecode.emit_u16(exclude_keys.len() as u16);
                for key in exclude_keys {
                    // Narrower than `struct_key`: an excluded key comes from a
                    // `&keys` pattern, which names fields and nothing else.
                    let key_value = match key {
                        LirConst::Keyword(hash) => Value::keyword_from_hash(*hash),
                        LirConst::Symbol(sid) => Value::symbol(*sid),
                        _ => panic!("StructRest: unsupported key type {:?}", key),
                    };
                    let const_idx = self.bytecode.add_constant(key_value);
                    self.bytecode.emit_u16(const_idx);
                }
                self.pop();
                self.push_reg(*dst);
            }

            // Silent destructuring (parameter context: absent optional params → nil)
            LirInstr::FirstOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::RestOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::ArrayMutRefOrNil);
                self.bytecode.emit_u16(*index);
                self.pop();
                self.push_reg(*dst);
            }

            _ => self.emit_instr_ops(instr),
        }
    }
}
