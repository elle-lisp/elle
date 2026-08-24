// Boot attribution rides the trace system: `--trace=boot` marks each boot
// phase (primitive registration, core, prelude, stdlib) and `--trace=compile`
// marks each pipeline phase, both as `[trace:...] <label> <N>ms` on stderr.
// The trap this pins: `--trace=` rejects unknown keywords, so a binary
// without the `boot` keyword fails these runs outright rather than running
// silently without marks.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run_traced(trace: &str, source: &str, tag: &str) -> (bool, String) {
    let dir = crate::common::ScratchDir::new(tag);
    let script = dir.join("script.lisp");
    std::fs::write(&script, source).expect("write script");
    let out = Command::new(elle_binary())
        .arg(format!("--trace={}", trace))
        .arg(&script)
        .output()
        .expect("run elle");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn boot_trace_marks_attribute_startup_phases() {
    let (ok, stderr) = run_traced("boot", "(+ 1 2)\n", "trace-boot");
    assert!(ok, "elle --trace=boot failed; stderr:\n{}", stderr);
    for label in [
        "primitives",
        "core",
        "prelude",
        "stdlib-compile",
        "stdlib-execute",
    ] {
        let line = stderr
            .lines()
            .find(|l| l.starts_with("[trace:boot]") && l.contains(label));
        let line = line.unwrap_or_else(|| {
            panic!(
                "missing [trace:boot] mark for phase '{}'; stderr:\n{}",
                label, stderr
            )
        });
        assert!(
            line.trim_end().ends_with("ms"),
            "boot mark for '{}' lacks a duration: {}",
            label,
            line
        );
    }
}

#[test]
fn compile_trace_times_pipeline_phases() {
    let (ok, stderr) = run_traced("compile", "(+ 1 2)\n", "trace-compile");
    assert!(ok, "elle --trace=compile failed; stderr:\n{}", stderr);
    for label in ["read", "expand", "analyze", "regions", "lower", "emit"] {
        let line = stderr
            .lines()
            .find(|l| l.starts_with("[trace:compile]") && l.contains(label));
        let line = line.unwrap_or_else(|| {
            panic!(
                "missing [trace:compile] mark for phase '{}'; stderr:\n{}",
                label, stderr
            )
        });
        assert!(
            line.trim_end().ends_with("ms"),
            "compile mark for '{}' lacks a duration: {}",
            label,
            line
        );
    }
}
