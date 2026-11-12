// audited: 2026-11-11
//! A tail call to a closure the JIT compiled runs the compiled code, whichever trampoline runs the callee.
//!
//! docs/impl/jit.md
//!
//! The callee's bytecode answers its argument, 1, and the code installed
//! for it answers 99, so the result says which of the two ran. The
//! counter-factual is a trampoline that interprets every tail callee: each test
//! then reads 1.

use std::sync::Arc;

use crate::config::JitPolicy;
use crate::jit::{JitCode, JitValue};
use crate::pipeline::compile_file_repl;
use crate::runtime::Runtime;
use crate::value::{Value, SIG_OK};

/// The code installed for the callee: 99, whatever it is passed.
extern "C" fn answers_99(
    _env: *const Value,
    _args: *const Value,
    _nargs: u32,
    _vm: *mut (),
    _self_tag: u64,
    _self_payload: u64,
) -> JitValue {
    JitValue::from_value(Value::int(99))
}

/// The program's answer, an array of what the tests call:
///
/// 0. the callee, `(fn [x] x)`;
/// 1. a caller that tail-calls its first argument on its second;
/// 2. the same tail call with its arguments spliced out of an array;
/// 3. an outer function that calls its first argument on the other two, in
///    call position, and answers the result in an array;
/// 4. the array `[1]` the spliced caller spreads.
const PROGRAM: &str = "[(fn [x] x) \
                        (fn [f x] (f x)) \
                        (fn [f xs] (f ;xs)) \
                        (fn [g f x] [(g f x)]) \
                        [1]]";

struct Fixture {
    rt: Runtime,
    program: Value,
    values: Vec<Value>,
}

impl Fixture {
    /// The program's values on a runtime with the JIT on, so a tail resolver
    /// asks the cache, at a threshold no test reaches, so nothing compiles
    /// but what a test compiles. The callee runs `answers_99` once compiled.
    fn new() -> Self {
        let mut rt = Runtime::without_stdlib();
        rt.vm().runtime_config.jit = JitPolicy::Adaptive {
            threshold: usize::MAX,
        };
        let prog = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(PROGRAM, symbols, cctx, "<tail-dispatch>")
                .expect("compiles")
                .0
        };
        let program = {
            let (vm, _symbols, cctx) = rt.parts();
            vm.execute_scheduled(&prog, cctx).expect("runs")
        };
        let values = program
            .array_to_vec()
            .expect("the program answers an array");
        let callee = values[0].as_closure().expect("the callee is a closure");
        rt.vm().install_jit_code(
            (*callee.template).clone(),
            Arc::new(JitCode::test_with_entry(answers_99)),
        );
        Fixture {
            rt,
            program,
            values,
        }
    }

    fn callee(&self) -> Value {
        self.values[0]
    }

    fn caller(&self) -> Value {
        self.values[1]
    }

    fn spliced(&self) -> Value {
        self.values[2]
    }

    fn outer(&self) -> Value {
        self.values[3]
    }

    fn one(&self) -> Value {
        self.values[4]
    }

    /// Run `f` on `args` interpreted, as a macro transformer runs.
    fn interpret(&mut self, f: Value, args: &[Value]) -> Value {
        self.rt
            .vm()
            .call_closure(f, args)
            .expect("the call answers")
    }

    /// Compile `f` and run it on `args` (`compile/run-on :jit`).
    fn compile_and_run(&mut self, f: Value, args: &[Value]) -> Value {
        let closure = f.as_closure().expect("a closure");
        let (bits, value) = self.rt.vm().invoke_closure_jit(f, closure, args);
        assert_eq!(bits, SIG_OK, "the compiled call answers");
        value
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::value::arena::release_program_value(self.rt.heap(), self.program);
    }
}

/// The interpreter's trampoline, reached from `tail_call_inner`.
#[test]
fn an_interpreted_tail_call_runs_the_callees_compiled_code() {
    let mut fx = Fixture::new();
    let (caller, callee) = (fx.caller(), fx.callee());
    let got = fx.interpret(caller, &[callee, Value::int(1)]);
    assert_eq!(got.as_int(), Some(99));
}

/// A spliced tail call reaches the same trampoline after the call has
/// reclaimed its args array.
#[test]
fn a_spliced_tail_call_runs_the_callees_compiled_code() {
    let mut fx = Fixture::new();
    let (spliced, callee, one) = (fx.spliced(), fx.callee(), fx.one());
    let got = fx.interpret(spliced, &[callee, one]);
    assert_eq!(got.as_int(), Some(99));
}

/// A compiled caller hands the tail call back with the sentinel, and
/// `compile/run-on :jit` runs it.
#[test]
fn a_compiled_tail_call_runs_the_callees_compiled_code() {
    let mut fx = Fixture::new();
    let (caller, callee) = (fx.caller(), fx.callee());
    let got = fx.compile_and_run(caller, &[callee, Value::int(1)]);
    assert_eq!(got.as_int(), Some(99));
}

/// The sentinel comes back to the interpreter's call into compiled code
/// (`run_jit`), from a compiled caller the interpreter calls.
#[test]
fn a_sentinel_back_to_the_interpreter_runs_the_compiled_callee() {
    let mut fx = Fixture::new();
    let (outer, caller, callee) = (fx.outer(), fx.caller(), fx.callee());
    fx.compile_and_run(caller, &[callee, Value::int(1)]);
    let got = first(fx.interpret(outer, &[caller, callee, Value::int(1)]));
    assert_eq!(got.as_int(), Some(99));
}

/// The sentinel comes back to a compiled call (`elle_jit_call`), from a
/// compiled caller a compiled function calls.
#[test]
fn a_sentinel_back_to_compiled_code_runs_the_compiled_callee() {
    let mut fx = Fixture::new();
    let (outer, caller, callee) = (fx.outer(), fx.caller(), fx.callee());
    fx.compile_and_run(caller, &[callee, Value::int(1)]);
    let got = first(fx.compile_and_run(outer, &[caller, callee, Value::int(1)]));
    assert_eq!(got.as_int(), Some(99));
}

/// The one element of the array the outer function answers.
fn first(answer: Value) -> Value {
    answer
        .array_to_vec()
        .expect("the outer function answers an array")[0]
}
