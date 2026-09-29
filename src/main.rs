// audited: 2026-09-29
//! The `elle` binary: dispatch a subcommand, or parse elle's flags and hand the
//! program to `elle::program`.
//!
//! docs/config.md

use elle::runtime::Runtime;
use std::env;

mod image_cli;
use image_cli::run_image;
mod help;
use help::print_help;
mod semver_cli;
use semver_cli::run_semver_subcommand;

/// The agent-first test runner, embedded at build time, in the order its
/// definitions are evaluated. See docs/test-runner.md.
const TEST_RUNNER_FRAGMENTS: &[&str] = &[
    include_str!("test/store.lisp"),
    include_str!("test/import.lisp"),
    include_str!("test/exec.lisp"),
    include_str!("test/record.lisp"),
    include_str!("test/view.lisp"),
    include_str!("test/main.lisp"),
];

/// The runner as one module.
///
/// Each fragment is a whole Elle file — formatted, stamped, and read on its
/// own — so each carries the epoch declaration a file needs. A module declares
/// its epoch once, so every fragment past the first drops the line here.
fn test_runner_source() -> String {
    let mut src = String::new();
    for (i, fragment) in TEST_RUNNER_FRAGMENTS.iter().enumerate() {
        for line in fragment.lines() {
            if i > 0 && line.starts_with("(elle/epoch") {
                continue;
            }
            src.push_str(line);
            src.push('\n');
        }
    }
    src
}

/// Split a subcommand's argv into elle's own flags — `--trace=`, `--boot-image=`
/// and `--dump=stats`, which configure the VM the subcommand runs on — and the
/// subcommand's. Each `(flag, n)` in `takes_values` keeps the `n` arguments after
/// it, whatever they spell.
pub(crate) fn split_own_flags(
    args: Vec<String>,
    takes_values: &[(&str, usize)],
) -> (Vec<String>, Vec<String>) {
    let mut own = Vec::new();
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if let Some((_, n)) = takes_values.iter().find(|(flag, _)| *flag == arg) {
            rest.push(arg);
            rest.extend(args.by_ref().take(*n));
        } else if arg.starts_with("--trace=")
            || arg.starts_with("--boot-image=")
            || arg == "--dump=stats"
        {
            own.push(arg);
        } else {
            rest.push(arg);
        }
    }
    (own, rest)
}

/// The runner's flags that take values (src/test/main.lisp).
const RUNNER_VALUE_FLAGS: &[(&str, usize)] = &[
    ("--db", 1),
    ("--timeout", 1),
    ("--wide", 1),
    ("--wide-timeout", 1),
    ("--isolate", 1),
    ("--host", 1),
    ("--corpus", 1),
    ("--import", 1),
    ("--query", 1),
    ("--promote", 2),
];

/// `elle test ...` — set up a full VM and run the embedded runner with the
/// post-`test` arguments exposed to it as the program argv (via `(sys/argv)`).
/// The runner calls `(os/exit ...)` itself with the gate code; the Ok/Err
/// mapping here is the fallback if it returns without exiting.
fn run_test_subcommand(sub_args: Vec<String>) -> i32 {
    // Split off elle's own flags (`--trace=...`, `--boot-image=...`,
    // `--dump=stats`) so the embedded runner's VM (and the off-VM free-log /
    // page-claim histogram) honour them; the rest become the runner's argv. A
    // runner flag's value is the runner's even when it spells one of elle's
    // flags: `--isolate '--trace=scrub'` hands the flag to each child, not to
    // this VM. `--boot-image=` boots this instance from an image, which is how
    // a suite is compiled against a hydrated stdlib (docs/impl/image/boot.md).
    let (config_flags, sub_args) = split_own_flags(sub_args, RUNNER_VALUE_FLAGS);
    let (config, _rest) = elle::config::Config::parse(&config_flags).unwrap_or_else(|e| {
        eprintln!("elle test: {}", e);
        std::process::exit(1);
    });
    elle::config::init(config);
    elle::io::init_process_signals();

    // One runtime, one teardown — the same lifecycle every entry path uses.
    let mut rt = Runtime::new();
    rt.vm().source_arg = "<test>".to_string();
    rt.vm().user_args = sub_args;

    let code = {
        let (vm, symbols, cctx) = rt.parts();
        match elle::program::run_source(&test_runner_source(), "src/test", vm, symbols, cctx) {
            Ok(_) => 0,
            Err(_) => 1,
        }
    };
    // The runner usually calls `(os/exit …)` itself (skipping Drop); on a
    // graceful return `rt`'s Drop runs the principled teardown sweep.
    code
}

fn main() {
    // dlopen'd C++ plugins (e.g. oxigraph) allocate from glibc's static TLS
    // block at load time. glibc 2.39+ grows that reservation on demand, so no
    // up-front reservation is needed here. If plugin loading ever fails with
    // "cannot allocate memory in static TLS block", set
    // GLIBC_TUNABLES=glibc.rtld.optional_static_tls=65536 before launching elle.

    let args: Vec<String> = env::args().collect();

    // Subcommand dispatch — no VM setup needed for these
    match args.get(1).map(|s| s.as_str()) {
        Some("fmt") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::formatter::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("lint") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::lint::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("lsp") => {
            let exit_code = elle::lsp::run::run();
            std::process::exit(exit_code);
        }
        Some("rewrite") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::rewrite::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("image") => {
            // A boot image is written by a full source boot, so this needs a
            // `Runtime` like `test` does rather than answering before VM init.
            let exit_code = run_image(&args[2..]);
            std::process::exit(exit_code);
        }
        Some("semver") => {
            // The gate is an Elle program (src/semver) and needs a full VM,
            // like `elle test`.
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = run_semver_subcommand(sub_args);
            std::process::exit(exit_code);
        }
        Some("test") => {
            // The runner is an Elle program (src/test). Unlike fmt/lint it
            // needs a full VM (sqlite FFI, stdlib, threads), so run the embedded
            // source with the post-`test` args handed to it as the program argv.
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = run_test_subcommand(sub_args);
            std::process::exit(exit_code);
        }
        _ => {}
    }

    // Interpreter mode — needs VM setup

    let (config, remaining_args) = elle::config::Config::parse(&args[1..]).unwrap_or_else(|e| {
        eprintln!("elle: {}", e);
        std::process::exit(1);
    });

    // --help and --version answer before VM init, so they still answer in a
    // tree whose stdlib or plugin is broken — which is when somebody asks. They
    // are elle's only before the program name, where `Config::parse` stops, so a
    // script carries a --help or a --version of its own.
    if config.help {
        print_help();
        return;
    }
    if config.version {
        println!("{}", elle::BANNER);
        return;
    }

    std::process::exit(elle::program::run(config, remaining_args));
}
