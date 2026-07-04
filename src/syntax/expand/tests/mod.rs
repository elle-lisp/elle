//! Tests for macro expansion

use super::*;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax, SyntaxKind};
use crate::vm::VM;
use std::cell::RefCell;

mod defmacro;
mod expand;
mod quasiquote;

fn setup() -> (Expander, SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    // Seed the expander's `eval_meta` with primitive metadata, mirroring
    // `CompileCtx::new`. `eval_syntax` (macro transformer-body compilation)
    // resolves primitives through `eval_meta`; a bare `Expander::new()` starts
    // empty, so a macro invocation would otherwise fail with "undefined
    // variable" on the first primitive in the transformer body.
    let mut expander = Expander::new();
    expander.set_eval_meta(crate::primitives::build_primitive_meta(&mut symbols));
    (expander, symbols, vm)
}
