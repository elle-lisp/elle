// audited: 2026-11-21
//! The program the compile passes build from an analyzed source, and the instance parts a compile reaches.
//!
//! docs/analysis/portrait.md
//!
//! The analysis stops after the analyzer. A closure's slots are fixed only
//! once the graft and the hoist have run, so the capture queries read this
//! program instead.

use super::*;
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::vm::VM;
use std::cell::OnceCell;

/// The program as the compile passes leave it: the tree the lowerer reads, in
/// an arena of its own.
pub struct Compiled {
    pub hir: Hir,
    pub arena: BindingArena,
}

/// An analysis's compiled program, built on first use. A source the compile
/// front end refuses keeps the error it raised.
#[derive(Default)]
pub struct CompiledCell(OnceCell<Result<Compiled, String>>);

impl CompiledCell {
    /// The program the compile passes build from `source`, read as
    /// `file_name`. `prim` names the query in an error.
    pub(in crate::primitives::compile) fn get(
        &self,
        source: &str,
        file_name: &str,
        prim: &str,
        ctx: &mut NativeCtx,
    ) -> Result<&Compiled, (SignalBits, Value)> {
        if self.0.get().is_none() {
            let (symbols, _vm, cctx) = compile_parts(prim, ctx)?;
            let built = crate::pipeline::compile_file_to_fhir(source, symbols, cctx, file_name)
                .map(|(hir, arena)| Compiled { hir, arena });
            let _ = self.0.set(built);
        }
        match self.0.get().expect("the cell was filled above") {
            Ok(compiled) => Ok(compiled),
            Err(e) => Err((
                SIG_ERROR,
                ctx.error("compile-error", format!("{}: {}", prim, e)),
            )),
        }
    }
}

/// The symbol table, the VM and the compile context of the instance the
/// native runs under. Each is a disjoint sibling in the owning `RuntimeCore`,
/// reached through the driving VM, so all three are separate `&mut`.
pub(in crate::primitives::compile) fn compile_parts<'a>(
    prim: &str,
    ctx: &mut NativeCtx,
) -> Result<(&'a mut SymbolTable, &'a mut VM, &'a mut CompileCtx), (SignalBits, Value)> {
    // `ctx.vm()` is total: a native always runs under a VM.
    let vm_ptr: *mut VM = ctx.vm();
    let symbols_ptr = unsafe { (*vm_ptr).symbols_ptr };
    if symbols_ptr.is_null() {
        let message = format!("{}: no symbol table in context", prim);
        return Err((SIG_ERROR, ctx.error("runtime-error", message)));
    }
    let cctx_ptr = unsafe { (*vm_ptr).compile_ctx_ptr };
    if cctx_ptr.is_null() {
        let message = format!("{}: no compile context", prim);
        return Err((SIG_ERROR, ctx.error("runtime-error", message)));
    }
    Ok(unsafe { (&mut *symbols_ptr, &mut *vm_ptr, &mut *cctx_ptr) })
}
