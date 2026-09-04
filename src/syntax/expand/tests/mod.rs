//! Tests for macro expansion

use super::*;
use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};
use crate::vm::VM;
use std::cell::RefCell;

mod defmacro;
mod expand;
mod quasiquote;

/// The four things every expander test needs: the expander, its symbol table,
/// the VM its transformers run on, and the working arena the expander and the
/// reader share.
fn setup() -> (Expander, SymbolTable, VM, SyntaxArena) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    let arena = SyntaxArena::mint(vm.heap());
    // Seed the expander's `eval_meta` with primitive metadata, mirroring
    // `CompileCtx::new`. `eval_syntax` (macro transformer-body compilation)
    // resolves primitives through `eval_meta`; a bare expander starts empty, so
    // a macro invocation would otherwise fail with "undefined variable" on the
    // first primitive in the transformer body.
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
    expander.set_eval_meta(crate::primitives::build_primitive_meta(&mut symbols));
    (expander, symbols, vm, arena)
}
