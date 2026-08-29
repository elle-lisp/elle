// `--trace` phase marks around the stdlib disk cache.
//
// The cache and the boot marks meet in `init_stdlib`: the cache exists to skip
// the stdlib compile, the marks exist to attribute it. Loading the stdlib is
// still the phase, so it is still marked.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run(args: &[&str], script: &std::path::Path) -> (bool, String) {
    let out = Command::new(elle_binary())
        .args(args)
        .arg(script)
        .output()
        .expect("run elle");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn script_in(dir: &crate::common::ScratchDir) -> std::path::PathBuf {
    let script = dir.join("script.lisp");
    std::fs::write(&script, "(+ 1 2)\n").expect("write script");
    script
}

#[test]
fn a_cache_hit_still_reports_the_stdlib_boot_phase() {
    let dir = crate::common::ScratchDir::new("trace-stdlib-cache");
    let script = script_in(&dir);
    let cache = format!("--cache={}", dir.join("cache").display());

    // The first run compiles and stores; the second loads what the first wrote.
    let (ok, first) = run(&[&cache, "--trace=boot"], &script);
    assert!(ok, "first run failed:\n{first}");
    let (ok, second) = run(&[&cache, "--trace=boot"], &script);
    assert!(ok, "second run failed:\n{second}");

    // The trap: the cache-hit path returns early, so a mark placed only on the
    // compile arm vanishes the moment the cache starts working. The
    // counter-factual is a second run whose stderr names no stdlib phase at
    // all — boot attribution quietly stops covering the slowest boot step, and
    // the only symptom is a shorter trace nobody reads twice.
    for (which, err) in [("first", &first), ("second", &second)] {
        assert!(
            err.lines()
                .any(|l| l.starts_with("[trace:boot]") && l.contains("stdlib-compile")),
            "{which} run reports no stdlib-compile boot mark:\n{err}"
        );
    }
    assert!(
        second.contains("stdlib-compile (cache hit)"),
        "the second run must load the stdlib the first one stored:\n{second}"
    );
}

#[test]
fn phase_marks_stay_quiet_without_the_keyword() {
    // A diagnostic that prints when nobody asked is a diagnostic that gets
    // filtered out and then ignored.
    let dir = crate::common::ScratchDir::new("trace-quiet");
    let script = script_in(&dir);
    let (ok, err) = run(&[], &script);
    assert!(ok, "elle exited non-zero:\n{err}");
    for subsystem in ["[trace:boot]", "[trace:compile]"] {
        assert!(
            !err.contains(subsystem),
            "no {subsystem} phase mark may appear without --trace:\n{err}"
        );
    }
}
