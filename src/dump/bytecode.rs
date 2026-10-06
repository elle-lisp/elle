// audited: 2026-10-06
//! `--dump=bytecode`: what the emitter wrote for a file, one code object at a time, in a text two runs agree on.
//!
//! docs/config.md
//!
//! The text is the bytecode golden's record, so every part of it is printed in
//! an order the emitter fixed rather than one a hash map chose, and no
//! constant prints an address.

use crate::compiler::bytecode::{disassemble_lines, Bytecode};
use crate::symbol::SymbolTable;
use crate::value::{TemplateProto, Value};
use std::fmt::Write;

/// Every code object a compiled file builds: the entry function, then each
/// nested lambda by its `MakeClosure` index path.
pub fn bytecode_unit(bc: &Bytecode, symbols: Option<&SymbolTable>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "; code object entry");
    let mut merged: Vec<u32> = bc.merged_slots.iter().copied().collect();
    merged.sort_unstable();
    let _ = writeln!(s, "  signal={:?}", bc.signal);
    tables(
        &mut s,
        &merged,
        &bc.frame_release_slots,
        &bc.frame_release_regions,
    );
    body(
        &mut s,
        &bc.instructions,
        &bc.constants,
        &bc.location_map,
        symbols,
    );
    for (i, child) in bc.child_protos.iter().enumerate() {
        proto(&mut s, &i.to_string(), child, symbols);
    }
    s
}

fn proto(s: &mut String, path: &str, p: &TemplateProto, symbols: Option<&SymbolTable>) {
    let _ = writeln!(
        s,
        "; code object [{path}] {} arity={} locals={} captures={} params={}",
        p.name.as_deref().unwrap_or("<anon>"),
        p.arity,
        p.num_locals,
        p.num_captures,
        p.num_params,
    );
    let _ = writeln!(
        s,
        "  signal={:?} vararg={:?} capture_params_mask=0x{:x} capture_locals={:?}",
        p.signal,
        p.vararg_kind,
        p.capture_params_mask,
        p.capture_locals_mask.words(),
    );
    let regions: Vec<u32> = p.region_table.iter().map(|r| r.get()).collect();
    let _ = writeln!(s, "  region_table={regions:?}");
    let mut merged: Vec<u32> = p.merged_slots.iter().copied().collect();
    merged.sort_unstable();
    tables(s, &merged, &p.frame_release_slots, &p.frame_release_regions);
    body(s, &p.bytecode, &p.constants, &p.location_map, symbols);
    if let Some(lir) = p.lir_function.as_ref() {
        let lir = lir.view();
        for yp in lir.yield_points() {
            let regs: Vec<u32> = yp.stack_regs.iter().map(|r| r.0).collect();
            let _ = writeln!(
                s,
                "  yield_point ip={} locals={} regs={regs:?}",
                yp.resume_ip, yp.num_locals
            );
        }
        for cs in lir.call_sites() {
            let regs: Vec<u32> = cs.stack_regs.iter().map(|r| r.0).collect();
            let _ = writeln!(
                s,
                "  call_site ip={} locals={} regs={regs:?}",
                cs.resume_ip, cs.num_locals
            );
        }
    }
    for (i, child) in p.child_protos.iter().enumerate() {
        proto(s, &format!("{path}.{i}"), child, symbols);
    }
}

fn tables(s: &mut String, merged: &[u32], release_slots: &[u16], release_regions: &[u32]) {
    let _ = writeln!(
        s,
        "  merged_slots={merged:?} release_slots={release_slots:?} release_regions={release_regions:?}"
    );
}

fn body(
    s: &mut String,
    code: &[u8],
    constants: &[Value],
    locations: &crate::error::LocationMap,
    symbols: Option<&SymbolTable>,
) {
    let _ = writeln!(s, "  bytecode ({} bytes):", code.len());
    for line in disassemble_lines(code) {
        let _ = writeln!(s, "    {line}");
    }
    let _ = writeln!(s, "  constants:");
    for (i, c) in constants.iter().enumerate() {
        let _ = writeln!(s, "    [{i}] {}", constant(*c, symbols));
    }
    // The emitter's map is a hash map, so its iteration order is not the
    // text's.
    let mut locs: Vec<(&usize, &crate::reader::SourceLoc)> = locations.iter().collect();
    locs.sort_unstable_by_key(|(off, _)| **off);
    let _ = writeln!(s, "  locations:");
    for (off, loc) in locs {
        let _ = writeln!(s, "    {off} {loc}");
    }
}

/// A constant by what it is, never by where it lives: a closure prints its
/// label and a native function its id, both of which name the same thing in
/// every run.
fn constant(v: Value, symbols: Option<&SymbolTable>) -> String {
    if let Some(c) = v.as_closure() {
        return format!("<closure {}>", c.template.display_label());
    }
    if v.is_native_fn() {
        return format!("<native-fn #{}>", v.payload);
    }
    format!("{}", v.debug_with(symbols))
}
