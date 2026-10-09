// audited: 2026-10-06
//! `--dump=bytecode`: what the emitter wrote for a file, one code object at a time, in a text two runs agree on.
//!
//! docs/config.md
//!
//! The text is the bytecode golden's record, so every part of it is printed in
//! an order the emitter fixed rather than one a hash map chose, and no
//! constant prints an address.

use crate::compiler::bytecode::disassemble_lines;
use crate::symbol::SymbolTable;
use crate::value::{ClosureTemplate, Value};
use std::fmt::Write;

/// Every code object a compiled file builds: the entry function, then each
/// nested lambda by its `MakeClosure` index path.
pub fn bytecode_unit(unit: &crate::value::CodeUnit, symbols: Option<&SymbolTable>) -> String {
    let entry = unit.entry();
    let mut s = String::new();
    let _ = writeln!(s, "; code object entry");
    let _ = writeln!(s, "  signal={:?}", entry.signal());
    tables(&mut s, entry);
    body(&mut s, entry, symbols);
    for i in 0..entry.num_children() {
        child(&mut s, &i.to_string(), &entry.child(i), symbols);
    }
    s
}

fn child(s: &mut String, path: &str, p: &ClosureTemplate, symbols: Option<&SymbolTable>) {
    let _ = writeln!(
        s,
        "; code object [{path}] {} arity={} locals={} captures={} params={}",
        p.name().unwrap_or("<anon>"),
        p.arity(),
        p.num_locals(),
        p.num_captures(),
        p.num_params(),
    );
    let _ = writeln!(
        s,
        "  signal={:?} vararg={:?} capture_params_mask=0x{:x} capture_locals={:?}",
        p.signal(),
        p.vararg_kind(),
        p.capture_params_mask(),
        p.capture_locals_mask().words(),
    );
    let regions: Vec<u32> = p.region_table().iter().map(|r| r.get()).collect();
    let _ = writeln!(s, "  region_table={regions:?}");
    tables(s, p);
    body(s, p, symbols);
    if let Some(lir) = p.lir() {
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
    for i in 0..p.num_children() {
        child(s, &format!("{path}.{i}"), &p.child(i), symbols);
    }
}

/// The merge set and the two release tables, each ascending as the payload
/// stores it.
fn tables(s: &mut String, p: &ClosureTemplate) {
    let _ = writeln!(
        s,
        "  merged_slots={:?} release_slots={:?} release_regions={:?}",
        p.merged_slots().as_slice(),
        p.frame_release_slots(),
        p.frame_release_regions(),
    );
}

fn body(s: &mut String, p: &ClosureTemplate, symbols: Option<&SymbolTable>) {
    let code = p.bytecode();
    let _ = writeln!(s, "  bytecode ({} bytes):", code.len());
    for line in disassemble_lines(code) {
        let _ = writeln!(s, "    {line}");
    }
    let _ = writeln!(s, "  constants:");
    for (i, c) in p.constants().iter().enumerate() {
        let _ = writeln!(s, "    [{i}] {}", constant(*c, symbols));
    }
    // The payload's location table is ascending by offset.
    let _ = writeln!(s, "  locations:");
    for (off, loc) in p.locations().iter() {
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
