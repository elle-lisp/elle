// audited: 2026-09-22
//! Where each form's bytes go, counted line by line.
//!
//! docs/impl/image/measurements.md
//!
//! The Rust side counts `capacity`, never `len`: a vector the lowerer pushed
//! into holds its growth slack until somebody shrinks it, and nobody does. The
//! region side counts what the slices name, and the page slack it does not
//! name is reported beside it.

use elle::lir::{LirConst, LirFunction, LirInstr};
use elle::value::ConstTemplate;

use crate::node::PFunc;

/// One row of the accounting: what it is, and how many bytes it holds.
pub struct Row {
    pub what: &'static str,
    pub bytes: usize,
}

/// Where the Rust-heap form's bytes go.
pub fn rust_rows(corpus: &[LirFunction]) -> Vec<Row> {
    use std::mem::size_of;
    let (mut shells, mut slack, mut blocks, mut operands) = (0, 0, 0, 0);
    let (mut tables, mut text, mut templates) = (0, 0, 0);

    for f in corpus {
        tables += size_of::<LirFunction>();
        blocks += f.blocks.capacity() * size_of::<elle::lir::BasicBlock>();
        for b in &f.blocks {
            let each = size_of::<elle::lir::SpannedInstr>();
            shells += b.instructions.len() * each;
            slack += (b.instructions.capacity() - b.instructions.len()) * each;
            for si in &b.instructions {
                operands += instr_operands(&si.instr);
                text += instr_text(&si.instr);
                templates += instr_template(&si.instr);
            }
        }
        tables += f.constants.capacity() * size_of::<LirConst>();
        for c in &f.constants {
            text += const_text(c);
        }
        tables += std::mem::size_of_val(f.capture_locals_mask.words());
        tables += f.region_table.capacity() * size_of::<u32>();
        tables += f.merged_slots.capacity() * size_of::<u32>();
        tables += f.frame_release_slots.capacity() * size_of::<u16>();
        tables += f.frame_release_regions.capacity() * size_of::<u32>();
        for y in &f.yield_points {
            tables += size_of::<elle::lir::YieldPointInfo>()
                + y.stack_regs.capacity() * size_of::<elle::lir::Reg>();
        }
        for c in &f.call_sites {
            tables += size_of::<elle::lir::CallSiteInfo>()
                + c.stack_regs.capacity() * size_of::<elle::lir::Reg>();
        }
        text += f.name.as_ref().map_or(0, |n| n.capacity());
        text += f.doc.as_ref().map_or(0, |d| d.len());
    }

    vec![
        Row {
            what: "instruction shells",
            bytes: shells,
        },
        Row {
            what: "instruction vector growth slack",
            bytes: slack,
        },
        Row {
            what: "block shells",
            bytes: blocks,
        },
        Row {
            what: "per-instruction operand vectors",
            bytes: operands,
        },
        Row {
            what: "constant templates",
            bytes: templates,
        },
        Row {
            what: "function tables",
            bytes: tables,
        },
        Row {
            what: "strings",
            bytes: text,
        },
    ]
}

/// How many instructions carry an operand vector.
///
/// `LirInstr` is as big as its largest variant, and the largest variant is
/// large because it carries two vectors. This is how many instructions that
/// size is chosen for; the rest pay it and use none of it.
pub fn vector_bearing(corpus: &[LirFunction]) -> usize {
    corpus
        .iter()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .filter(|si| instr_operands(&si.instr) > 0)
        .count()
}

/// The register and key vectors one instruction owns.
fn instr_operands(i: &LirInstr) -> usize {
    use std::mem::size_of;
    let regs = size_of::<elle::lir::Reg>();
    match i {
        LirInstr::MakeClosure { captures, .. } => captures.capacity() * regs,
        LirInstr::Call { args, .. } | LirInstr::SuspendingCall { args, .. } => {
            args.capacity() * regs
        }
        LirInstr::TailCall {
            args,
            borrowed_arg_slots,
            ..
        } => args.capacity() * regs + borrowed_arg_slots.capacity() * size_of::<u16>(),
        LirInstr::MakeArrayMut { elements, .. } => elements.capacity() * regs,
        LirInstr::FreeRegionGroup { members } => members.capacity() * regs,
        LirInstr::PushParamFrame { pairs } => pairs.capacity() * 2 * regs,
        LirInstr::StructRest { exclude_keys, .. } => {
            exclude_keys.capacity() * size_of::<LirConst>()
        }
        _ => 0,
    }
}

/// The string bytes one instruction's constants own.
fn instr_text(i: &LirInstr) -> usize {
    match i {
        LirInstr::Const { value, .. }
        | LirInstr::StructGetOrNil { key: value, .. }
        | LirInstr::StructGetDestructure { key: value, .. } => const_text(value),
        LirInstr::StructRest { exclude_keys, .. } => exclude_keys.iter().map(const_text).sum(),
        _ => 0,
    }
}

fn const_text(c: &LirConst) -> usize {
    match c {
        LirConst::String(s) => s.capacity(),
        _ => 0,
    }
}

/// The tree one `MaterializeConst` owns.
fn instr_template(i: &LirInstr) -> usize {
    match i {
        LirInstr::MaterializeConst { template, .. } => template_bytes(template),
        _ => 0,
    }
}

fn template_bytes(t: &ConstTemplate) -> usize {
    use std::mem::size_of;
    let node = size_of::<ConstTemplate>();
    match t {
        ConstTemplate::Symbol(s) | ConstTemplate::Keyword(s) => node + s.capacity(),
        ConstTemplate::String(s) | ConstTemplate::StringMut(s) => node + s.capacity(),
        ConstTemplate::Pair(a, b) => node + template_bytes(a) + template_bytes(b),
        ConstTemplate::Array(items) | ConstTemplate::ArrayMut(items) => {
            node + items.iter().map(template_bytes).sum::<usize>()
        }
        ConstTemplate::SyntaxSymbol { name, scopes, .. } => {
            node + name.capacity() + scopes.capacity() * size_of::<u32>()
        }
        _ => node,
    }
}

/// Where the region form's bytes go.
pub fn region_rows(built: &[PFunc]) -> Vec<Row> {
    use std::mem::size_of;
    let (mut shells, mut blocks, mut pool, mut consts) = (0, 0, 0, 0);
    let (mut templates, mut tables) = (0, 0);
    for f in built {
        tables += size_of::<PFunc>();
        blocks += f.blocks.len() * size_of::<crate::node::PBlock>();
        for b in f.blocks.iter() {
            shells += b.instrs.len() * size_of::<crate::node::PInstr>();
        }
        pool += f.pool.len() * size_of::<u32>();
        consts += f.consts.len() * size_of::<crate::node::PConst>();
        templates += f.templates.len() * size_of::<crate::node::PTemplate>();
        tables += (f.yield_points.len() + f.call_sites.len()) * size_of::<crate::node::PSite>();
        tables += f.capture_locals.len() * size_of::<u64>();
        tables += (f.region_table.len() + f.merged_slots.len() + f.frame_release_regions.len())
            * size_of::<u32>();
        tables += f.frame_release_slots.len() * size_of::<u16>();
        tables += f.name.as_str().len();
    }
    vec![
        Row {
            what: "instruction shells",
            bytes: shells,
        },
        Row {
            what: "block shells",
            bytes: blocks,
        },
        Row {
            what: "operand pool",
            bytes: pool,
        },
        Row {
            what: "constants",
            bytes: consts,
        },
        Row {
            what: "constant templates",
            bytes: templates,
        },
        Row {
            what: "function tables",
            bytes: tables,
        },
    ]
}
