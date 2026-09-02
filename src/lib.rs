#![allow(clippy::result_large_err)]

//! # Elle - A High-Performance Lisp Interpreter
//!
//! Elle is a bytecode-compiled Lisp interpreter written in Rust with a register-based VM.
//!
//! ## Quick Start
//!
//! The recommended embedding entry point is [`Runtime`](runtime::Runtime): it
//! installs the contexts, registers primitives, loads the stdlib, and — on drop
//! — runs the principled, RC-driven process-teardown sweep (docs/impl/region/rules.md
//! § "Teardown — every region frees"), the same lifecycle `elle foo.lisp` and the REPL use.
//!
//! ```
//! use elle::pipeline::eval;
//! use elle::runtime::Runtime;
//!
//! let mut rt = Runtime::new();
//! // The instance's three disjoint capabilities: the VM, the symbol table, and
//! // its own compile state. Two `Runtime`s on one thread are isolated — each
//! // names its own here.
//! let (vm, symbols, cctx) = rt.parts();
//! let result = eval("(+ 1 2 3)", symbols, vm, cctx, "<example>").unwrap();
//! // `rt` dropping here tears the runtime down by RC; only the native-fn
//! // primitives (immediates, no region) persist. `rt.teardown()` runs it
//! // explicitly and returns the observable region census.
//! ```
//!
//! The lower-level pieces remain available for embedders that manage the VM,
//! symbol table, and compile context themselves (no automatic teardown):
//!
//! ```
//! use elle::{eval, init_stdlib, register_primitives, SymbolTable, VM};
//! use elle::pipeline::CompileCtx;
//!
//! let mut vm = VM::new();
//! let mut symbols = SymbolTable::new();
//! register_primitives(&mut vm, &mut symbols);
//! // Compile-time state and the symbol table are explicit per-instance
//! // capabilities. Point the VM at both (so runtime `eval`/`import` and value
//! // name-resolution resolve against THIS instance) and thread them through
//! // every pipeline call.
//! let mut cctx = CompileCtx::new();
//! vm.set_compile_ctx(&mut cctx as *mut CompileCtx);
//! vm.set_symbols(&mut symbols as *mut SymbolTable);
//! init_stdlib(&mut vm, &mut symbols, &mut cctx);
//!
//! let code = "(+ 1 2 3)";
//! let result = eval(code, &mut symbols, &mut vm, &mut cctx, "<example>").unwrap();
//! ```
//!
//! ## Architecture
//!
//! Elle compiles Lisp code through several stages:
//!
//! 1. **Reader** - Parse S-expressions from text
//! 2. **Compiler** - Convert AST to bytecode
//! 3. **VM** - Execute bytecode with a stack-based interpreter
//!
//! ## Performance
//!
//! - Bytecode compilation eliminates tree-walking overhead
//! - Register-based VM for efficient instruction dispatch
//! - Symbol interning for O(1) symbol comparison
//! - SmallVec optimization to avoid heap allocation

// No custom global allocator. Arena pages use mmap directly (bypassing
// the global allocator entirely), and the remaining allocations (tracking
// vecs, mutable collections, Rc boxes) are moderate-throughput enough
// that the system allocator is sufficient.

/// The elle version. Single source of truth: `[package] version` in the root
/// `Cargo.toml`. Every user-visible version string (REPL banner, `--help`
/// header, LSP `serverInfo`, the `(elle/version)` primitive) must derive from
/// this constant so a release bumps exactly one line.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The user-facing banner line derived from [`VERSION`].
pub const BANNER: &str = concat!("Elle v", env!("CARGO_PKG_VERSION"));

#[macro_use]
pub mod trace;
pub mod arithmetic;
pub mod compiler;
pub mod config;
pub mod dump;
pub mod epoch;
pub mod error;
pub mod ffi;
pub mod formatter;
pub mod hir;
pub mod image;
pub mod io;
#[cfg(feature = "jit")]
pub mod jit;
pub mod lint;
pub mod lir;
pub mod lsp;
#[cfg(feature = "mlir")]
pub mod mlir;
pub mod path;
pub mod pipeline;
pub mod plugin;
#[allow(improper_ctypes_definitions)]
pub mod plugin_api;
pub mod port;
// `#[macro_use]`: the `type-error` macros of `primitives::arg` also serve the
// VM opcode handlers, and `vm` is declared after this.
#[macro_use]
pub mod primitives;
pub mod reader;
pub mod repl;
pub mod rewrite;
pub mod runtime;
pub mod segment;
pub mod signals;
pub mod symbol;
pub mod symbols;
pub mod syntax;
pub mod value;
pub mod vm;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use compiler::Bytecode;
pub use error::SourceLoc;
pub use lint::{
    cli::{LintConfig, Linter, OutputFormat},
    diagnostics::{Diagnostic, Severity},
};
pub use pipeline::{
    analyze, analyze_file, compile, compile_file, eval, eval_all, eval_file, AnalyzeResult,
    CompileResult,
};
pub use primitives::{init_stdlib, register_primitives};
pub use reader::{read_str, Lexer, Reader};
pub use symbol::SymbolTable;
pub use symbols::{SymbolDef, SymbolIndex, SymbolKind};
pub use value::Value;
pub use vm::VM;
