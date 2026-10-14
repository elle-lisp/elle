// audited: 2026-10-14
// A macro body reads and changes nothing outside its unit: the primitives an expansion refuses, and a file's compile.
//
// docs/macros.md

use crate::common::eval_source;

/// Every primitive that reaches state outside the unit through no capability
/// bit, by its canonical name. A primitive joins the list when its body reads
/// or changes process state, a VM cache or an address; one that needs a handle
/// only such a primitive mints is left off.
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
    "ffi/run-teardowns",
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

/// The canonical names `vm/primitive-meta` marks `:ambient`, sorted and joined
/// by spaces.
const MARKED: &str = r#"
(def names @[])
(each name in (vm/list-primitives)
  (let [meta (vm/primitive-meta name)]
    (when (and meta (get meta :ambient))
      (push names (get meta :name)))))
(string/join (sort (distinct (freeze names))) " ")
"#;

/// The mark is a claim the expansion gate acts on, so the list is read whole:
/// a primitive gains or loses it only by an edit here. The counter-factual
/// marks nothing, and every ambient read reaches a macro body.
#[test]
fn the_primitives_an_expansion_refuses_are_these() {
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

/// A silent transformer that reads the pid reports the refusal, not a broken
/// proof: the refusal halts, and a halt crosses a silent boundary. The
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

/// A closure the transformer forces onto the bytecode tier halts on the refusal
/// as well, and the compile reports it. The counter-factual expanded to the
/// pid.
#[test]
fn a_forced_tier_reports_the_refusal() {
    let source = "(defmacro pid [] (compile/run-on :bytecode (fn [] (sys/pid))))\n(pid)\n";
    let err = eval_source(source, |r| r.expect_err("the compile fails"));
    assert!(
        err.contains("macro 'pid'") && err.contains("sys/pid"),
        "the error names the macro and the primitive: {err}"
    );
}
