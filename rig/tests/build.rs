// audited: 2026-10-04
// The rig names the build it is, and runs a program it is given; it has no REPL.
// rig/overview.md
// docs/ratchet.md
//
// The counter-factual: `(elle/build)` lived in every user build and answered a
// struct a program could branch on, so the shipped runtime carried a primitive
// for the test suite's convenience. These read the key off the rig alone.

mod common;

use common::{rig, stderr, stdout};
use std::process::{Command, Stdio};

/// What `(elle/build)` answers on this rig.
fn key() -> String {
    let out = rig(&["-e", "(println (elle/build))"]);
    assert!(
        out.status.success(),
        "`elle-rig -e (elle/build)` fails:\n{}",
        stderr(&out)
    );
    stdout(&out).trim().to_string()
}

#[test]
fn the_build_key_names_the_tier_the_backend_and_the_platform() {
    let key = key();
    let parts: Vec<&str> = key.split('-').collect();
    assert_eq!(parts.len(), 4, "tier-backend-os-arch, got {key:?}");
    assert!(
        ["wasm", "mlir", "jit", "interp"].contains(&parts[0]),
        "the tier the rig carries, got {key:?}"
    );
    assert!(
        ["uring", "pool"].contains(&parts[1]),
        "the I/O backend it runs, got {key:?}"
    );
    assert_eq!(
        parts[2],
        std::env::consts::OS,
        "the operating system, got {key:?}"
    );
    assert_eq!(
        parts[3],
        std::env::consts::ARCH,
        "the architecture, got {key:?}"
    );
}

/// The default features on Linux x86_64 are the reference build, the one a row
/// with no `:build` belongs to, and the ledger module names the same key.
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "jit",
    feature = "uring",
    not(feature = "mlir"),
    not(feature = "wasm")
))]
#[test]
fn the_reference_build_is_the_default_build_on_linux_x86_64() {
    assert_eq!(key(), "jit-uring-linux-x86_64");
    let out = rig(&[
        "-e",
        "(println (get ((import \"std/ratchet/ledger\")) :reference-build))",
    ]);
    assert_eq!(
        stdout(&out).trim(),
        "jit-uring-linux-x86_64",
        "the ledger module names the reference build:\n{}",
        stderr(&out)
    );
}

#[test]
fn the_rig_refuses_to_run_with_no_program() {
    // The trap: with no program `elle` starts its REPL, which reads stdin; a
    // rig that did the same would wait on whatever stdin its caller left
    // open. The null stdin keeps this test from hanging on such a rig.
    let out = Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .stdin(Stdio::null())
        .output()
        .expect("spawn elle-rig");
    assert_eq!(
        out.status.code(),
        Some(2),
        "the rig has no REPL, so it refuses a run with no program:\n{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("elle-rig:"),
        "and says why:\n{}",
        stderr(&out)
    );
}
