// JIT compilation integration tests
//
// These tests verify that the JIT compiler correctly translates LIR to native
// code and produces the same results as the interpreter.

use elle::jit::{JitCompiler, JitError};
use elle::lir::{
    BasicBlock, BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, SpannedInstr,
    SpannedTerminator, Terminator, UnaryOp,
};
use elle::signals::Signal;
use elle::syntax::Span;
use elle::value::{Arity, Value};

// Local `eval`/`compile` shims preserving the pre-CompileCtx arity. Every site
// here registers primitives only (no stdlib) and never evaluates the `(eval …)`
// or `(import …)` runtime special forms, so a fresh `CompileCtx` per call
// (primitives + core + prelude) reproduces the old bare-symbols path exactly —
// no compile state needs to persist across calls, and the VM never reaches the
// cctx through its runtime pointer. The cctx is dropped after the call returns,
// which is safe precisely because nothing retains a pointer into it.
fn eval(
    source: &str,
    symbols: &mut elle::symbol::SymbolTable,
    vm: &mut elle::vm::VM,
    source_name: &str,
) -> Result<Value, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::eval(source, symbols, vm, &mut cctx, source_name)
}

// Sibling of `eval` above for parity; kept for tests that compile without
// evaluating. Not all build configurations exercise it.
#[allow(dead_code)]
fn compile(
    source: &str,
    symbols: &mut elle::symbol::SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

// `eval`/`compile` variants that load the stdlib first, so library functions
// (abs, append, reverse, map, range, push, …) resolve. Loading points the VM at
// the symbol table (stdlib macros gensym, resolving through it) and builds a
// `CompileCtx` that carries the stdlib exports into the compile that follows.
// Heavier than the bare shims above (it compiles + runs stdlib.lisp on `vm`), so
// only the library-using tests below use these.
fn stdlib_cctx(
    symbols: &mut elle::symbol::SymbolTable,
    vm: &mut elle::vm::VM,
) -> elle::pipeline::CompileCtx {
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_symbols(symbols as *mut elle::symbol::SymbolTable);
    elle::init_stdlib(vm, symbols, &mut cctx);
    cctx
}

fn eval_with_stdlib(
    source: &str,
    symbols: &mut elle::symbol::SymbolTable,
    vm: &mut elle::vm::VM,
    source_name: &str,
) -> Result<Value, String> {
    let mut cctx = stdlib_cctx(symbols, vm);
    elle::pipeline::eval(source, symbols, vm, &mut cctx, source_name)
}

// Compile-only variant (signature matches the bare `compile`): the stdlib must
// be in `cctx.meta` for name resolution, but the export closures only need to
// exist on a throwaway VM — nothing here executes them. The throwaway VM also
// gives `symbols` its primitives (a fresh table otherwise lacks them).
fn compile_with_stdlib(
    source: &str,
    symbols: &mut elle::symbol::SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut vm = elle::vm::VM::new();
    let _ = elle::register_primitives(&mut vm, symbols);
    let mut cctx = stdlib_cctx(symbols, &mut vm);
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

// =============================================================================
// Helper Functions
// =============================================================================

fn span() -> Span {
    Span::synthetic()
}

/// Create a LoadCapture instruction to load an argument into a register.
/// With num_captures=0, LoadCapture index N loads from args[N].
fn load_arg(dst: Reg, arg_index: u16) -> SpannedInstr {
    SpannedInstr::new(
        LirInstr::LoadCapture {
            dst,
            index: arg_index,
        },
        span(),
    )
}

fn compile_and_call(lir: &LirFunction, args: &[Value]) -> Result<Value, JitError> {
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    // A real VM is required: every compiled function's Cranelift prologue calls
    // `elle_jit_push_region_map(vm)` (and the body's alloc/decref helpers deref
    // `vm`), so a null vm pointer faults. Mirror the runtime entry `VM::call_jit`
    // (src/vm/jit_entry.rs) — `VM::new()` sets up the VM's heap and the prologue
    // pushes this activation's region map, so a fresh VM is a complete context.
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let compiler = JitCompiler::new()?;
    let code = compiler.compile(lir, None, std::collections::HashMap::new(), Vec::new())?;
    // self_tag/self_payload = 0 since we're not testing self-tail-calls in these basic tests
    let result = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            args.len() as u32,
            &mut vm as *mut VM as *mut (),
            0,
            0,
        )
    };
    Ok(result.to_value())
}

mod basics {
    include!("jit/basics.rs");
}
mod arithmetic {
    include!("jit/arithmetic.rs");
}
mod control {
    include!("jit/control.rs");
}
mod bitwise {
    include!("jit/bitwise.rs");
}
mod data {
    include!("jit/data.rs");
}
mod tailcall {
    include!("jit/tailcall.rs");
}
mod fastpath {
    include!("jit/fastpath.rs");
}
mod recursion {
    include!("jit/recursion.rs");
}
mod letrec {
    include!("jit/letrec.rs");
}
mod rotation {
    include!("jit/rotation.rs");
}
