// audited: 2026-10-22
// A macro body reads and changes nothing outside its unit: the primitives that declare :ambient, a file's compile, a host primitive.
//
// docs/macros.md

use crate::common::eval_source;
use elle::primitives::ctx::NativeCtx;
use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::types::Arity;
use elle::Value;

/// Every primitive that declares `:ambient`, by its canonical name. A primitive
/// joins the list when its body reads or changes process state, a VM cache or
/// an address; one that needs a handle only such a primitive mints is left off.
const AMBIENT: &[&str] = &[
    "allocator/install",
    "allocator/uninstall",
    "backend?",
    "clock/cpu",
    "clock/monotonic",
    "clock/realtime",
    "debug/arena-adopts",
    "debug/arena-adopts-into-empty",
    "debug/arena-allocs",
    "debug/arena-bytes",
    "debug/arena-count",
    "debug/arena-dump",
    "debug/arena-empty-frees",
    "debug/arena-extracts",
    "debug/arena-few-object-frees",
    "debug/arena-many-object-frees",
    "debug/arena-object-frees",
    "debug/arena-object-limit",
    "debug/arena-one-object-frees",
    "debug/arena-one-page-frees",
    "debug/arena-over-frees",
    "debug/arena-owned",
    "debug/arena-owned-free-objects",
    "debug/arena-owned-free-pages",
    "debug/arena-owned-frees",
    "debug/arena-owned-one-page-frees",
    "debug/arena-page-claims",
    "debug/arena-page-frees",
    "debug/arena-peak",
    "debug/arena-region-count",
    "debug/arena-region-frees",
    "debug/arena-region-ids",
    "debug/arena-region-info",
    "debug/arena-region-of",
    "debug/arena-region-table",
    "debug/arena-reparents",
    "debug/arena-rescue-survivors",
    "debug/arena-rescues",
    "debug/arena-reset-peak",
    "debug/arena-set-object-limit",
    "debug/arena-stats",
    "debug/arena-total-allocs",
    "debug/memory",
    "debug/print",
    "debug/symbol-count",
    "debug/trace",
    "disgit",
    "elle/executable",
    "fiber/parent",
    "fn/disasm-jit",
    "fn/git?",
    "hash",
    "io/backend",
    "io/cancel",
    "io/reap",
    "io/wait",
    "io/workers",
    "jit/rejections",
    "jit?",
    "os/sig-close",
    "os/sig-mask",
    "os/sig-pending",
    "os/sig-watch",
    "os/sig-watching",
    "port/set-options",
    "port/stderr",
    "port/stdin",
    "port/stdout",
    "pp",
    "signals",
    "sys/argv",
    "sys/args",
    "sys/env",
    "sys/exit",
    "sys/pid",
    "sys/spawn",
    "sys/spawn-vm",
    "sys/thread-id",
    "sys/trap-exit!",
    "sys/unique",
    "vm/config",
    "vm/config-set",
    "vm/query",
    "vm/tier",
    "watch",
    "watch-close",
];

/// The canonical names whose declared bits `vm/primitive-meta` answers with
/// `:ambient`, sorted and joined by spaces.
const MARKED: &str = r#"
(def names @[])
(each name in (vm/list-primitives)
  (let [meta (vm/primitive-meta name)]
    (when (and meta (contains? (get meta :bits) :ambient))
      (push names (get meta :name)))))
(string/join (sort (distinct (freeze names))) " ")
"#;

/// The bit is a claim every denial acts on, so the list is read whole: a
/// primitive gains or loses it only by an edit here. The counter-factual
/// declares it nowhere, and every ambient read reaches a macro body.
#[test]
fn the_primitives_that_declare_ambient_are_these() {
    let mut expected: Vec<&str> = AMBIENT.to_vec();
    if cfg!(feature = "mlir") {
        expected.push("mlir/compile-spirv");
    }
    expected.sort_unstable();
    let marked = eval_source(MARKED, |r| {
        r.map(|v| {
            v.with_string(str::to_string)
                .expect("the names as a string")
        })
    })
    .expect("the census runs");
    let marked: Vec<&str> = marked.split(' ').filter(|s| !s.is_empty()).collect();
    assert_eq!(marked, expected);
}

/// The file a program runs compiles before any fiber exists, and its macros
/// still lose the effect capabilities. The counter-factual expanded the macro
/// with every capability, and the file answered the manifest's text.
#[test]
fn a_file_compile_expands_without_the_effect_capabilities() {
    let source = "(defmacro manifest [] (file/read \"Cargo.toml\"))\n(manifest)\n";
    let err = eval_source(source, |r| r.expect_err("the compile fails"));
    assert!(
        err.contains("macro 'manifest'") && err.contains("file/read"),
        "the error names the macro and the primitive: {err}"
    );
}

/// A transformer that reads the pid reports the denial, not a broken proof: the
/// transformer carries :ambient, so no silent boundary stands in the way. The
/// counter-factual expanded to the pid of the test process.
#[test]
fn a_file_compile_refuses_an_ambient_read() {
    let source = "(defmacro pid [] (sys/pid))\n(pid)\n";
    let err = eval_source(source, |r| r.expect_err("the compile fails"));
    assert!(
        err.contains("macro 'pid'") && err.contains("sys/pid"),
        "the error names the macro and the primitive: {err}"
    );
    assert!(
        !err.contains("broken"),
        "no broken proof is reported: {err}"
    );
}

/// A closure the transformer forces onto the bytecode tier is denied as well,
/// and the compile reports it. The counter-factual expanded to the pid.
#[test]
fn a_forced_tier_reports_the_refusal() {
    let source = "(defmacro pid [] (compile/run-on :bytecode (fn [] (sys/pid))))\n(pid)\n";
    let err = eval_source(source, |r| r.expect_err("the compile fails"));
    assert!(
        err.contains("macro 'pid'") && err.contains("sys/pid"),
        "the error names the macro and the primitive: {err}"
    );
}

fn prim_host_answer(_ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::int(42))
}

/// A primitive the host registers after boot, as the rig registers
/// `elle/build`.
static HOST_ANSWER: PrimitiveDef = PrimitiveDef {
    name: "host/answer",
    func: prim_host_answer,
    signal: Signal::silent(),
    arity: Arity::Exact(0),
    doc: "Answer 42, the primitive a host registers in this test.",
    params: &[],
    category: "host",
    example: "(host/answer)",
    effect: RegionEffect::Immediate,
    ..PrimitiveDef::DEFAULT
};

/// A macro body reads a host primitive as run time does: it is listed, it has
/// metadata and a docstring, and it runs. The counter-factual registered the
/// primitive's documentation on the program VM alone, and the body found
/// neither the name nor the docstring.
#[test]
fn a_macro_body_sees_a_host_primitive() {
    let source = r#"
(defmacro seen []
  (string/join
    (map string
         [(contains? (apply set (vm/list-primitives)) 'host/answer)
          (if-let [meta (vm/primitive-meta "host/answer")] (get meta :name) "no-meta")
          (string/contains? (doc host/answer) "Answer 42")
          (host/answer)])
    " "))
(seen)
"#;
    let mut rt = elle::runtime::Runtime::new();
    rt.register_primitive(&HOST_ANSWER);
    let (vm, symbols, cctx) = rt.parts();
    let seen = elle::eval_all(source, symbols, vm, cctx, "<test>").map(|v| {
        v.with_string(str::to_string)
            .expect("the expansion is a string")
    });
    assert_eq!(seen, Ok("true host/answer true 42".to_string()));
}
