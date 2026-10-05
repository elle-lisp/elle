// audited: 2026-10-05
//! A signal that leaves a silent function is a broken proof: the terminal
//! `:broken-proof` error, whatever the signal and wherever the call sits.
//!
//! docs/signals/inference.md
//!
//! Inference is sound, so Elle source cannot build a silent function that
//! signals. Each test compiles one that signals and forges a copy of its code
//! object whose recorded signal is silent, which is what a defect in the
//! compiler would hand the VM.

use crate::pipeline::eval_syntax;
use crate::primitives::{build_primitive_meta, register_primitives};
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::{Expander, SyntaxArena};
use crate::value::arena::with_test_region;
use crate::value::closure::{CodeArena, PayloadParts, TemplateRef};
use crate::value::{Closure, SignalBits, Value};
use crate::value::{SIG_ERROR, SIG_HALT, SIG_TERMINAL};
use crate::vm::VM;

/// A VM with the primitives and the prelude, and the means to compile on it.
struct Bench {
    vm: VM,
    symbols: SymbolTable,
    expander: Expander,
    arena: SyntaxArena,
}

impl Bench {
    fn new() -> Self {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        let arena = SyntaxArena::mint(vm.heap());
        let mut expander = Expander::on_vm(&mut vm);
        expander.set_arena(arena);
        expander.set_eval_meta(build_primitive_meta(&mut symbols));
        expander.load_prelude(&mut symbols, &mut vm).unwrap();
        Bench {
            vm,
            symbols,
            expander,
            arena,
        }
    }

    /// The closure `source` evaluates to.
    fn closure(&mut self, source: &str) -> Value {
        let syntax = crate::reader::read_syntax(self.arena, source, "<proof>").unwrap();
        let value =
            eval_syntax(syntax, &mut self.expander, &mut self.symbols, &mut self.vm).unwrap();
        assert!(value.as_closure().is_some(), "{source} is a closure");
        value
    }

    /// A copy of `closure` whose code object records a silent signal: the
    /// same bytecode under a proof it does not keep. `name` labels it.
    fn forge_silent(&mut self, closure: Value, name: &str) -> Value {
        let original = closure.as_closure().unwrap();
        let forged = PayloadParts {
            signal: Signal::silent(),
            name: Some(name.to_string()),
            ..PayloadParts::of_payload(&original.template)
        };
        let heap = unsafe { &mut *self.vm.heap_ptr };
        // The code region keeps the reference it was minted with, so the
        // forged code object lives as long as the heap.
        let code = CodeArena::mint(heap);
        let template = TemplateRef::region(code.header(code.write(forged)));
        let region = heap.new_runtime_region();
        crate::value::build::closure(
            heap,
            Closure::new(template, original.env, SignalBits::EMPTY),
            region,
        )
    }

    /// Run `f` on `args` as the root body, and answer the signal it left
    /// with and that signal's payload.
    fn run(&mut self, f: Value, args: &[Value]) -> (SignalBits, Value) {
        let closure = f.as_closure().unwrap();
        let env = self
            .vm
            .build_closure_env(closure, args)
            .expect("the arguments fit");
        self.vm.pending_entry_closure = f;
        let exit = self
            .vm
            .execute_bytecode_saving_stack(&closure.template.code(), &env);
        let (_, payload) = self
            .vm
            .fiber
            .signal
            .take()
            .unwrap_or((exit.bits, Value::NIL));
        (exit.bits, payload)
    }
}

/// The terminal error a broken proof raises, read as `kind: message`.
fn broken_proof(bits: SignalBits, payload: Value) -> String {
    assert_eq!(
        bits,
        SIG_ERROR.union(SIG_TERMINAL),
        "a broken proof is an error no mask catches; got {bits} carrying {}",
        crate::value::error::format_error(payload)
    );
    crate::value::error::format_error(payload)
}

/// A yield out of a silent callee, called where its result is still needed.
/// Counter-factual: the boundary treated the silent function as a squelch
/// over every signal, and raised a catchable `:signal-violation`.
#[test]
fn a_yield_from_a_silent_callee_is_a_broken_proof() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let yields = bench.closure("(fn [] (yield 1) 2)");
        let forged = bench.forge_silent(yields, "forged-yield");
        let caller = bench.closure("(fn [g] (def r (g)) r)");
        let (bits, payload) = bench.run(caller, &[forged]);
        assert_eq!(
            broken_proof(bits, payload),
            "broken-proof: broken proof: forged-yield is inferred silent but raised {:yield}"
        );
    })
}

/// An error out of a silent callee breaks the proof too: a caller depends on
/// a silent function not raising. Counter-factual: `:error` passed the
/// silent boundary as it passes a squelch, and reached the caller as the
/// callee's own error.
#[test]
fn an_error_from_a_silent_callee_is_a_broken_proof() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let raises = bench.closure("(fn [] (error :boom))");
        let forged = bench.forge_silent(raises, "forged-error");
        let caller = bench.closure("(fn [g] (def r (g)) r)");
        let (bits, payload) = bench.run(caller, &[forged]);
        assert_eq!(
            broken_proof(bits, payload),
            "broken-proof: broken proof: forged-error is inferred silent but raised {:error}"
        );
    })
}

/// A silent callee reached by a tail call has replaced its caller's frame,
/// so the boundary the activation ends at is the callee's. Counter-factual:
/// the tail path carried the squelch mask alone and lost the proof.
#[test]
fn a_yield_from_a_silent_tail_callee_is_a_broken_proof() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let yields = bench.closure("(fn [] (yield 1) 2)");
        let forged = bench.forge_silent(yields, "forged-tail");
        let caller = bench.closure("(fn [g] (g))");
        let (bits, payload) = bench.run(caller, &[forged]);
        assert_eq!(
            broken_proof(bits, payload),
            "broken-proof: broken proof: forged-tail is inferred silent but raised {:yield}"
        );
    })
}

/// A halt is a resource fault, not a broken proof: the depth cap halts a
/// deep recursion that no inference can see. It passes the silent boundary
/// as itself.
#[test]
fn a_halt_passes_the_boundary_of_a_silent_callee() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let halts = bench.closure("(fn [] (emit :halt 7))");
        let forged = bench.forge_silent(halts, "forged-halt");
        let caller = bench.closure("(fn [g] (def r (g)) r)");
        let (bits, payload) = bench.run(caller, &[forged]);
        assert_eq!(bits, SIG_HALT, "the halt leaves as itself");
        assert_eq!(payload, Value::int(7), "with its own value");
    })
}

/// A broken proof that passes a second silent boundary keeps the name of
/// the function whose proof broke first. Counter-factual: the outer
/// boundary raised a broken proof of its own over the inner one, and the
/// report named the wrong function.
#[test]
fn a_broken_proof_passes_an_outer_silent_boundary_whole() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let yields = bench.closure("(fn [] (yield 1) 2)");
        let inner = bench.forge_silent(yields, "forged-inner");
        let calls = bench.closure("(fn [h] (def r (h)) r)");
        let outer = bench.forge_silent(calls, "forged-outer");
        let caller = bench.closure("(fn [g h] (def r (g h)) r)");
        let (bits, payload) = bench.run(caller, &[outer, inner]);
        assert_eq!(
            broken_proof(bits, payload),
            "broken-proof: broken proof: forged-inner is inferred silent but raised {:yield}"
        );
    })
}

/// No mask catches a broken proof, so `protect`, whose fiber masks `:error`,
/// never sees it. The bench loads no stdlib, so the catch is spelled with the
/// fiber primitives `protect` expands to.
/// Counter-factual: the violation was an ordinary error, and the program's
/// own handler read the compiler's defect as its own failure.
#[test]
fn protect_does_not_catch_a_broken_proof() {
    with_test_region(|| {
        let mut bench = Bench::new();
        let yields = bench.closure("(fn [] (yield 1) 2)");
        let forged = bench.forge_silent(yields, "forged-protected");
        let caller =
            bench.closure("(fn [g] (fiber/resume (fiber/new (fn [] (def r (g)) r) |:error|)))");
        let (bits, payload) = bench.run(caller, &[forged]);
        assert_eq!(
            broken_proof(bits, payload),
            "broken-proof: broken proof: forged-protected is inferred silent but raised {:yield}"
        );
    })
}
