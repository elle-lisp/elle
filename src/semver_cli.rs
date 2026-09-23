// audited: 2026-09-23
//! `elle semver ...`: set up one full VM and run the embedded gate driver.
//!
//! docs/semver.md

use elle::runtime::Runtime;

/// The semver gate, embedded at build time. See docs/semver.md.
const SEMVER_RUNNER: &str = include_str!("semver/main.lisp");

/// `elle semver ...` — the same lifecycle as `elle test`: one full VM, the
/// embedded driver, the post-`semver` arguments as the program argv. The
/// driver calls `(os/exit ...)` itself; an uncaught error maps to 2, the
/// driver's tool-error code, so a crash never reads as a verdict.
pub(super) fn run_semver_subcommand(sub_args: Vec<String>) -> i32 {
    let (config_flags, sub_args): (Vec<String>, Vec<String>) = sub_args
        .into_iter()
        .partition(|a| a.starts_with("--trace=") || a == "--stats" || a == "--no-uring");
    let (config, _rest) = elle::config::Config::parse(&config_flags).unwrap_or_else(|e| {
        eprintln!("elle semver: {}", e);
        std::process::exit(2);
    });
    elle::config::init(config);
    elle::io::init_process_signals();

    let mut rt = Runtime::new();
    rt.vm().source_arg = "<semver>".to_string();
    rt.vm().user_args = sub_args;

    let (vm, symbols, cctx) = rt.parts();
    match crate::run_source(SEMVER_RUNNER, "src/semver", vm, symbols, cctx) {
        Ok(_) => 0,
        Err(_) => 2,
    }
}
