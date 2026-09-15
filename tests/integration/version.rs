// audited: 2026-09-14
// Version derivation tests.
//
// The version has exactly one source of truth: `[package] version` in the
// root Cargo.toml, surfaced as `elle::VERSION` / `elle::BANNER`. These tests
// pin every user-visible version string to that constant, so a release bump
// that misses a hardcoded banner fails here instead of shipping stale.
//
// docs/config.md

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn version_flag_prints_the_banner_and_nothing_else() {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("--version")
        .output()
        .expect("failed to spawn elle");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "--version must exit 0, got {:?}, stderr: {}",
        out.status.code(),
        stderr
    );
    assert_eq!(
        stdout.trim(),
        elle::BANNER,
        "--version prints the banner alone, got: {:?}",
        stdout
    );
}

#[test]
fn version_answers_before_the_file_argument_is_read() {
    // The trap: fold --version into the normal flag parse and the binary
    // builds a Runtime, reads the file, and reports that error first. A tree
    // whose stdlib or plugin is broken is exactly when somebody asks what
    // version they are holding, so the answer cannot depend on a working VM.
    //
    // The counter-factual: answer --version after the file loop instead of
    // before it, and this exits 1 with a "No such file" line on stderr while
    // stdout stays empty.
    let missing = std::env::temp_dir().join(format!(
        "elle-version-probe-{}-{:?}.lisp",
        std::process::id(),
        std::thread::current().id()
    ));
    assert!(!missing.exists(), "the probe path must not exist");
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("--version")
        .arg(&missing)
        .output()
        .expect("failed to spawn elle");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "--version must exit 0 whatever follows it, got {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.trim(),
        elle::BANNER,
        "--version answers before the file is read, got: {:?}",
        stdout
    );
}

#[test]
fn help_banner_derives_from_package_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("--help")
        .output()
        .expect("failed to spawn elle");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(elle::BANNER),
        "expected {:?} in --help output, got: {}",
        elle::BANNER,
        stdout
    );
}

#[test]
fn repl_banner_derives_from_package_version() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn elle");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"(exit)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(elle::BANNER),
        "expected {:?} in REPL greeting, got: {}",
        elle::BANNER,
        stdout
    );
}

#[test]
fn lsp_server_info_derives_from_package_version() {
    let (mut stdin, mut reader, mut child) = super::lsp::start_lsp();
    let result = super::lsp::init_lsp(&mut stdin, &mut reader);
    assert_eq!(result["serverInfo"]["name"], "Elle Language Server");
    assert_eq!(
        result["serverInfo"]["version"],
        elle::VERSION,
        "LSP serverInfo.version must derive from Cargo.toml"
    );
    super::lsp::shutdown_lsp(stdin, &mut reader, &mut child);
}
