// audited: 2026-10-06
//! The operations each form is measured on, written twice.
//!
//! docs/impl/image/measurements.md
//!
//! Each pair reads and writes the same information through the representation
//! under test. Where the two differ, the Rust-heap side does less: its walk
//! reads no `aux` or flag word, because reading one costs a per-variant match
//! the region form does not need.

use elle::lir::{for_each_def, for_each_use, LirConst, LirFunction, LirInstr};
use elle::value::region_slice::RegionSlice;
use elle::value::Value;

use crate::node::{PConst, PFunc};
use crate::opcode::opcode;

/// A region slice as a mutable slice.
///
/// The prototype mutates uniquely owned working copies, which is the condition
/// the syntax migration also landed under: a form built for this pass, read by
/// nobody else.
///
/// # Safety
/// No other reference to the slice's backing may be live.
unsafe fn as_mut<T: 'static>(s: RegionSlice<T>) -> &'static mut [T] {
    std::slice::from_raw_parts_mut(s.as_ptr() as *mut T, s.len())
}

/// Read every instruction the way a backend's translator does: the opcode, the
/// registers it defines and uses, its region slot, and its span.
pub fn walk_rust(f: &LirFunction) -> u64 {
    let mut sum = 0u64;
    for b in &f.blocks {
        sum = sum
            .wrapping_add(b.label.0 as u64)
            .wrapping_add(b.terminator.span.line as u64);
        for si in &b.instructions {
            sum = sum
                .wrapping_add(opcode(&si.instr) as u64)
                .wrapping_add(si.span.line as u64)
                .wrapping_add(si.instr.region().map(|r| r.get()).unwrap_or(0) as u64);
            for_each_def(&si.instr, |r| sum = sum.wrapping_add(r.0 as u64));
            for_each_use(&si.instr, |r| sum = sum.wrapping_add(r.0 as u64));
        }
    }
    sum
}

/// The same read over the region form.
pub fn walk_region(f: &PFunc) -> u64 {
    let mut sum = 0u64;
    let pool = f.pool.as_slice();
    for b in f.blocks.iter() {
        sum = sum
            .wrapping_add(b.label as u64)
            .wrapping_add(b.span.line as u64);
        for i in b.instrs.iter() {
            sum = sum
                .wrapping_add(i.op as u64)
                .wrapping_add(i.span.line as u64)
                .wrapping_add(i.region as u64)
                .wrapping_add(i.aux as u64)
                .wrapping_add(i.flags as u64);
            if i.dst != crate::node::NO_REG {
                sum = sum.wrapping_add(i.dst as u64);
            }
            match i.n_uses {
                0 => {}
                1 => sum = sum.wrapping_add(i.use0 as u64),
                2 => {
                    sum = sum.wrapping_add(i.use0 as u64).wrapping_add(i.use1 as u64);
                }
                n => {
                    sum = sum.wrapping_add(i.use0 as u64).wrapping_add(i.use1 as u64);
                    let at = i.extra as usize;
                    for k in 0..(n as usize - 2) {
                        sum = sum.wrapping_add(pool[at + k] as u64);
                    }
                }
            }
        }
    }
    sum
}

/// An in-place pass over every `ValueConst`: each one whose value [`immediate`]
/// spells becomes a `Const`. What is measured is the write where the
/// instruction lies, not the conversion behind it.
pub fn rewrite_rust(f: &mut LirFunction) -> u64 {
    let mut hits = 0u64;
    for b in &mut f.blocks {
        for si in &mut b.instructions {
            if let LirInstr::ValueConst { dst, value } = &si.instr {
                if let Some(c) = immediate(*value) {
                    si.instr = LirInstr::Const {
                        dst: *dst,
                        value: c,
                    };
                    hits += 1;
                }
            }
        }
    }
    hits
}

/// Build each function the way the lowerer does: a fresh vector per block, an
/// instruction pushed at a time. The region side's own build reads the same
/// corpus, so the two pay the same traversal.
pub fn build_rust(corpus: &[LirFunction]) -> Vec<LirFunction> {
    let mut out = Vec::with_capacity(corpus.len());
    for f in corpus {
        let mut nf = shell(f);
        for b in &f.blocks {
            let mut nb = elle::lir::BasicBlock::new(b.label);
            for si in &b.instructions {
                nb.instructions.push(si.clone());
            }
            nb.terminator = b.terminator.clone();
            nf.blocks.push(nb);
        }
        out.push(nf);
    }
    out
}

/// `f`'s header on a function with no blocks yet: what the lowerer has set by
/// the time it fills the first block.
pub fn shell(f: &LirFunction) -> LirFunction {
    let mut nf = LirFunction::new(f.arity);
    nf.closure_id = f.closure_id;
    nf.name = f.name.clone();
    nf.entry = f.entry;
    nf.constants = f.constants.clone();
    nf.num_regs = f.num_regs;
    nf.num_locals = f.num_locals;
    nf.num_captures = f.num_captures;
    nf.capture_params_mask = f.capture_params_mask;
    nf.capture_locals_mask = f.capture_locals_mask.clone();
    nf.signal = f.signal;
    nf.doc = f.doc.clone();
    nf.origin = f.origin;
    nf.vararg_kind = f.vararg_kind.clone();
    nf.rest_list_layout = f.rest_list_layout;
    nf.num_params = f.num_params;
    nf.num_local_params = f.num_local_params;
    nf.region_table = f.region_table.clone();
    nf.merged_slots = f.merged_slots.clone();
    nf.frame_release_slots = f.frame_release_slots.clone();
    nf.frame_release_regions = f.frame_release_regions.clone();
    nf
}

/// The same rewrite over the region form: the opcode byte and the pooled
/// constant, both written where they lie.
pub fn rewrite_region(f: &PFunc) -> u64 {
    let mut hits = 0u64;
    let consts = unsafe { as_mut(f.consts) };
    for b in f.blocks.iter() {
        for i in unsafe { as_mut(b.instrs) } {
            if i.op == 1 {
                if let PConst::Val(v) = consts[i.aux as usize] {
                    if let Some(c) = immediate(v) {
                        consts[i.aux as usize] = lir_to_pconst(&c);
                        i.op = 0;
                        hits += 1;
                    }
                }
            }
        }
    }
    hits
}

/// The constant the rewrite writes for a value: the immediate a scalar spells,
/// and a nil stand-in for a closure, so that the pass has as many sites as
/// there are closure-valued `ValueConst`s too.
fn immediate(v: Value) -> Option<LirConst> {
    if v.is_nil() {
        Some(LirConst::Nil)
    } else if v.is_empty_list() {
        Some(LirConst::EmptyList)
    } else if let Some(b) = v.as_bool() {
        Some(LirConst::Bool(b))
    } else if let Some(n) = v.as_int() {
        Some(LirConst::Int(n))
    } else if let Some(f) = v.as_float() {
        Some(LirConst::Float(f))
    } else if let Some(id) = v.as_symbol() {
        Some(LirConst::Symbol(id))
    } else if let Some(hash) = v.keyword_hash() {
        Some(LirConst::Keyword(hash))
    } else {
        v.is_closure().then_some(LirConst::Nil)
    }
}

/// A converted constant, for the one rewrite above. [`immediate`] writes no
/// `String`.
fn lir_to_pconst(c: &LirConst) -> PConst {
    match c {
        LirConst::Nil => PConst::Nil,
        LirConst::EmptyList => PConst::EmptyList,
        LirConst::Bool(b) => PConst::Bool(*b),
        LirConst::Int(n) => PConst::Int(*n),
        LirConst::Float(f) => PConst::Float(*f),
        LirConst::Symbol(s) => PConst::Symbol(s.0),
        LirConst::Keyword(k) => PConst::Keyword(*k),
        LirConst::String(_) => PConst::Nil,
    }
}

/// How many instructions and register operands a function holds, for the check
/// that the two forms carry the same graph.
pub fn census_rust(f: &LirFunction) -> (usize, usize) {
    let mut instrs = 0;
    let mut uses = 0;
    for b in &f.blocks {
        instrs += b.instructions.len();
        for si in &b.instructions {
            for_each_use(&si.instr, |_| uses += 1);
        }
    }
    (instrs, uses)
}

/// The same census over the region form.
pub fn census_region(f: &PFunc) -> (usize, usize) {
    let mut instrs = 0;
    let mut uses = 0;
    for b in f.blocks.iter() {
        instrs += b.instrs.len();
        for i in b.instrs.iter() {
            uses += i.n_uses as usize;
        }
    }
    (instrs, uses)
}
