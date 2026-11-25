// audited: 2026-11-25
//! Reads that take a value apart: a pair's halves, an array element or slice, a struct field or key's presence, and the silent forms.
//!
//! docs/impl/bytecode.md

use super::*;

/// The constant a struct or table key lowers to.
///
/// The pattern and access-path lowerers build only keyword and symbol keys, so
/// a string one never reaches the emitter — a constant pool has no region, and
/// a string is a heap value that would need one.
fn struct_key(key: &ConstRef, op: Instruction) -> Value {
    match key {
        ConstRef::Keyword(hash) => Value::keyword_from_hash(*hash),
        ConstRef::Int(n) => Value::int(*n),
        ConstRef::Symbol(sym) => Value::symbol(*sym),
        ConstRef::Bool(b) => Value::bool(*b),
        ConstRef::Nil => Value::NIL,
        _ => panic!("{op:?}: unsupported key type"),
    }
}

impl Emitter {
    /// A read of one struct key: `op`, then the key's constant-pool index.
    fn emit_keyed_read(&mut self, dst: Reg, src: Reg, key: &ConstRef, op: Instruction) {
        self.ensure_on_top(src);
        let const_idx = self.bytecode.add_constant(struct_key(key, op));
        self.bytecode.emit(op);
        self.bytecode.emit_u16(const_idx);
        self.pop();
        self.push_reg(dst);
    }

    /// The destructuring reads (chain link from `emit_instr`).
    pub(super) fn emit_instr_destructure(&mut self, instr: &InstrRef<'_>) {
        match instr {
            InstrRef::First { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::First);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::Rest { dst, pair } => {
                self.ensure_on_top(*pair);
                self.bytecode.emit(Instruction::Rest);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::MatchFail { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::MatchFail);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::FirstDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::RestDestructure { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestDestructure);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::ArrayMutRefDestructure { dst, src, index } => {
                self.emit_indexed(Instruction::ArrayMutRefDestructure, *dst, *src, *index)
            }
            InstrRef::ArraySliceFrom { dst, src, index } => {
                self.emit_indexed(Instruction::ArraySliceFrom, *dst, *src, *index)
            }
            InstrRef::ArrayMutSliceFrom { dst, src, index } => {
                self.emit_indexed(Instruction::ArrayMutSliceFrom, *dst, *src, *index)
            }

            InstrRef::StructGetOrNil { dst, src, key } => {
                self.emit_keyed_read(*dst, *src, key, Instruction::StructGetOrNil)
            }

            InstrRef::StructHasKey { dst, src, key } => {
                self.emit_keyed_read(*dst, *src, key, Instruction::StructHasKey)
            }

            InstrRef::StructGetDestructure { dst, src, key } => {
                self.emit_keyed_read(*dst, *src, key, Instruction::StructGetDestructure)
            }

            InstrRef::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::StructRest);
                self.bytecode.emit_u16(exclude_keys.len() as u16);
                for key in exclude_keys.iter() {
                    // Narrower than `struct_key`: an excluded key comes from a
                    // `&keys` pattern, which names fields and nothing else.
                    let key_value = match key {
                        ConstRef::Keyword(hash) => Value::keyword_from_hash(hash),
                        ConstRef::Symbol(sid) => Value::symbol(sid),
                        _ => panic!("StructRest: unsupported key type {:?}", key),
                    };
                    let const_idx = self.bytecode.add_constant(key_value);
                    self.bytecode.emit_u16(const_idx);
                }
                self.pop();
                self.push_reg(*dst);
            }

            // Silent destructuring (parameter context: absent optional params → nil)
            InstrRef::FirstOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::FirstOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::RestOrNil { dst, src } => {
                self.ensure_on_top(*src);
                self.bytecode.emit(Instruction::RestOrNil);
                self.pop();
                self.push_reg(*dst);
            }

            InstrRef::ArrayMutRefOrNil { dst, src, index } => {
                self.emit_indexed(Instruction::ArrayMutRefOrNil, *dst, *src, *index)
            }

            _ => self.emit_instr_ops(instr),
        }
    }

    /// An array read whose one operand is the index after the opcode: `src`
    /// on top of the stack in, `dst` out.
    fn emit_indexed(&mut self, op: Instruction, dst: Reg, src: Reg, index: u16) {
        self.ensure_on_top(src);
        self.bytecode.emit(op);
        self.bytecode.emit_u16(index);
        self.pop();
        self.push_reg(dst);
    }
}
