// audited: 2026-09-29
// What the rig reads from a sidecar and a profile, what it refuses, and what a
// running file sees.
// rig/overview.md

mod common;

use common::{lines, rig, stderr, stdout, Scratch};

/// A file that exits 3 whenever anything runs it. `--print-config` must not.
const EXITS: &str = "(elle/epoch 13)\n(os/exit 3)\n";

/// A file that prints what the running VM reports for `key`.
fn reads(key: &str) -> String {
    format!("(elle/epoch 13)\n(println (vm/config {key}))\n")
}

/// The configuration the rig prints for `prog.lisp` beside `sidecar`, with
/// `extra` flags before the path.
fn printed(dir: &Scratch, sidecar: Option<&str>, extra: &[&str]) -> Vec<String> {
    let path = dir.write("prog.lisp", EXITS);
    if let Some(body) = sidecar {
        dir.write("prog.toml", body);
    }
    let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    args.push("--print-config".to_string());
    args.push(path.display().to_string());
    let out = rig(&args);
    assert!(
        out.status.success(),
        "--print-config must print and exit 0, got {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        stdout(&out),
        stderr(&out)
    );
    lines(&out)
}

/// Assert that the rig refused `sidecar` before running anything, and named
/// `needle` in its reason.
fn assert_refused(sidecar: &str, needle: &str) {
    let dir = Scratch::new("refuse");
    let path = dir.write("prog.lisp", "(elle/epoch 13)\n(println \"ran\")\n");
    dir.write("prog.toml", sidecar);
    let out = rig(&[path]);
    assert!(
        !out.status.success(),
        "the rig must refuse the sidecar {sidecar:?}, and it exited 0"
    );
    assert!(
        !stdout(&out).contains("ran"),
        "the rig refused {sidecar:?} after running the file:\n{}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains(needle),
        "refusing {sidecar:?} must name {needle:?}, got:\n{}",
        stderr(&out)
    );
}

// ── Reading a sidecar ──

/// `--print-config` answers the configuration a file would run under, one line
/// per setting, and runs nothing. The file exits 3, so a rig that ran it
/// anyway exits non-zero here.
#[test]
fn print_config_reports_the_sidecar_and_runs_nothing() {
    let dir = Scratch::new("print");
    let got = printed(&dir, Some("jit = \"off\"\ntrace = [\"guardfree\"]\n"), &[]);
    assert!(
        got.contains(&"jit = \"off\"".to_string()),
        "the sidecar's jit setting, got {got:?}"
    );
    assert!(
        got.contains(&"trace = [\"guardfree\"]".to_string()),
        "the sidecar's trace keywords, got {got:?}"
    );
}

/// A file with no sidecar runs under the build's defaults: the runtime `elle`
/// ships, with nothing traced.
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
#[test]
fn a_file_with_no_sidecar_runs_under_the_build_defaults() {
    let dir = Scratch::new("defaults");
    let got = printed(&dir, None, &[]);
    assert!(got.contains(&"jit = 10".to_string()), "got {got:?}");
    assert!(got.contains(&"trace = []".to_string()), "got {got:?}");
}

/// A threshold is a positive integer, and an eager tier is the word.
#[test]
fn a_threshold_and_eager_read_back_as_written() {
    let dir = Scratch::new("threshold");
    let got = printed(&dir, Some("jit = 5\n"), &[]);
    assert!(got.contains(&"jit = 5".to_string()), "got {got:?}");
    let got = printed(&dir, Some("jit = \"eager\"\n"), &[]);
    assert!(got.contains(&"jit = \"eager\"".to_string()), "got {got:?}");
}

/// What `--print-config` prints is itself a sidecar this build accepts, and
/// reading it back gives the same configuration. The counter-factual: a
/// printer and a reader written apart drift, and a failing file's printed
/// configuration no longer reproduces the run.
#[test]
fn the_printed_configuration_is_a_sidecar_the_rig_accepts() {
    let dir = Scratch::new("roundtrip");
    let first = printed(
        &dir,
        Some("jit = \"eager\"\ntrace = [\"scrub\", \"guardfree\"]\n"),
        &[],
    );
    let again = printed(&dir, Some(&(first.join("\n") + "\n")), &[]);
    assert_eq!(
        first, again,
        "a printed configuration must read back unchanged"
    );
}

// ── Refusals ──

/// A misspelled key that the rig ignored would run the file under the defaults
/// and report a pass: the vacuous result the suite exists to prevent.
#[test]
fn an_unknown_key_is_refused_by_name() {
    assert_refused("jitt = \"off\"\n", "jitt");
}

#[test]
fn a_value_of_the_wrong_type_is_refused() {
    assert_refused("jit = true\n", "jit");
    assert_refused("jit = \"sometimes\"\n", "jit");
    assert_refused("trace = \"guardfree\"\n", "trace");
}

/// A count below one names no threshold.
#[test]
fn a_threshold_below_one_is_refused() {
    assert_refused("jit = 0\n", "jit");
    assert_refused("jit = -3\n", "jit");
}

/// A misspelled trace keyword is the same vacuous pass as a misspelled key:
/// the file runs with the oracle it asked for disarmed.
#[test]
fn a_trace_keyword_the_build_does_not_know_is_refused() {
    assert_refused("trace = [\"gaurdfree\"]\n", "gaurdfree");
}

/// A sidecar that does not parse as TOML is refused, never read as empty.
#[test]
fn a_sidecar_that_is_not_toml_is_refused() {
    assert_refused("jit = = off\n", "prog.toml");
}

#[cfg(not(feature = "mlir"))]
#[test]
fn an_mlir_key_is_refused_in_a_build_without_the_mlir_tier() {
    assert_refused("mlir = \"eager\"\n", "mlir");
}

#[cfg(all(feature = "mlir", not(feature = "wasm")))]
#[test]
fn an_mlir_key_sets_the_mlir_tier_in_an_mlir_build() {
    let dir = Scratch::new("mlir");
    let got = printed(&dir, Some("mlir = \"eager\"\n"), &[]);
    assert!(got.contains(&"mlir = \"eager\"".to_string()), "got {got:?}");
}

// ── Profiles ──

/// A profile's tier setting replaces the sidecar's, and its trace keywords
/// join the sidecar's. The counter-factual for each half: a profile that
/// replaced the trace set would disarm the guardfree oracle a sidecar armed,
/// and one that deferred to the sidecar's tier would leave a pinned file out of
/// the eager pass.
#[test]
fn a_profile_replaces_the_tier_and_joins_the_trace() {
    let dir = Scratch::new("profile");
    let profile = dir.write("eager.toml", "jit = \"eager\"\ntrace = [\"scrub\"]\n");
    let profile = profile.display().to_string();
    let got = printed(
        &dir,
        Some("jit = \"off\"\ntrace = [\"guardfree\"]\n"),
        &["--profile", &profile],
    );
    assert!(got.contains(&"jit = \"eager\"".to_string()), "got {got:?}");
    assert!(
        got.contains(&"trace = [\"guardfree\", \"scrub\"]".to_string()),
        "the two trace sets join, sorted, got {got:?}"
    );
}

/// A profile is read with the same rules as a sidecar.
#[test]
fn a_profile_with_an_unknown_key_is_refused() {
    let dir = Scratch::new("badprofile");
    let path = dir.write("prog.lisp", "(elle/epoch 13)\n(println \"ran\")\n");
    let profile = dir.write("bad.toml", "eagerly = true\n");
    let out = rig(&[
        "--profile".to_string(),
        profile.display().to_string(),
        path.display().to_string(),
    ]);
    assert!(!out.status.success(), "a bad profile must refuse the run");
    assert!(!stdout(&out).contains("ran"));
    assert!(stderr(&out).contains("eagerly"), "got:\n{}", stderr(&out));
}

// ── What a running file sees ──

/// Run `prog.lisp` reading `key`, beside `sidecar`, and answer its stdout.
fn running(sidecar: Option<&str>, key: &str) -> String {
    let dir = Scratch::new("running");
    let path = dir.write("prog.lisp", &reads(key));
    if let Some(body) = sidecar {
        dir.write("prog.toml", body);
    }
    let out = rig(&[path]);
    assert!(
        out.status.success(),
        "the rig failed to run the file: {:?}\nstderr:\n{}",
        out.status,
        stderr(&out)
    );
    stdout(&out).trim().to_string()
}

/// `(vm/config :jit)` reads the threshold: nil when the JIT is off, 0 when the
/// rig makes it eager, and the count otherwise. This is the only way a program
/// learns what the sidecar set, so it is what proves the setting reached the
/// VM rather than stopping at the printer.
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
#[test]
fn a_running_file_reads_the_jit_setting() {
    assert_eq!(running(None, ":jit"), "10");
    assert_eq!(running(Some("jit = \"off\"\n"), ":jit"), "nil");
    assert_eq!(running(Some("jit = \"eager\"\n"), ":jit"), "0");
    assert_eq!(running(Some("jit = 5\n"), ":jit"), "5");
}

#[test]
fn a_running_file_reads_the_trace_keywords() {
    assert_eq!(
        running(Some("trace = [\"guardfree\"]\n"), ":trace"),
        "|:guardfree|"
    );
}

/// The rig runs a file through the same path `elle` does, so a gated file
/// prints the line the runner reads and exits 0.
#[test]
fn a_gated_file_prints_the_skip_line() {
    let dir = Scratch::new("gated");
    let path = dir.write(
        "gated.lisp",
        "(elle/epoch 13)\n(error (struct :error :gated :reason \"no widget here\"))\n",
    );
    let out = rig(&[path]);
    assert!(out.status.success(), "a gated file exits 0");
    assert!(
        stderr(&out).contains("SKIP (gated): no widget here"),
        "got:\n{}",
        stderr(&out)
    );
}

/// A failing assertion fails the run and names itself, exactly as under
/// `elle`.
#[test]
fn a_failing_file_exits_non_zero_and_names_the_assertion() {
    let dir = Scratch::new("fails");
    let path = dir.write(
        "fails.lisp",
        "(elle/epoch 13)\n(assert false \"the rig ran this\")\n",
    );
    let out = rig(&[path]);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{}", stderr(&out));
    assert!(
        stderr(&out).contains("the rig ran this"),
        "got:\n{}",
        stderr(&out)
    );
}

/// Every argument after the program belongs to the program, flags included.
#[test]
fn arguments_after_the_program_belong_to_the_program() {
    let dir = Scratch::new("args");
    let path = dir.write("args.lisp", "(elle/epoch 13)\n(print (sys/args))\n");
    let out = rig(&[
        path.display().to_string(),
        "--profile".to_string(),
        "x".to_string(),
    ]);
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "(--profile x)");
}
