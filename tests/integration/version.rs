// Version derivation tests.
//
// The version has exactly one source of truth: `[package] version` in the
// root Cargo.toml, surfaced as `elle::VERSION` / `elle::BANNER`. These tests
// pin every user-visible version string to that constant, so a release bump
// that misses a hardcoded banner fails here instead of shipping stale.

use std::io::Write;
use std::process::{Command, Stdio};

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
