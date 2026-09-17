// audited: 2026-09-17
//! The `elle` binary: dispatch a subcommand, or set up one `Runtime` and drive
//! it from a file, `-e`, stdin or the REPL.
//!
//! docs/config.md

use elle::pipeline::{compile_file, CompileCtx};
use elle::repl::Repl;
use elle::runtime::Runtime;
use elle::{SymbolTable, VM};
use std::env;
use std::fs;
use std::io::{self, Read};

mod dump_cli;
use dump_cli::run_dump;
mod help;
use help::print_help;
mod errors;
use errors::{format_error_json, format_runtime_error, parse_compilation_error};

fn run_stdin(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> Result<(), String> {
    let mut contents = String::new();
    io::stdin().read_to_string(&mut contents).map_err(|e| {
        let msg = format!("Failed to read stdin: {}", e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    run_source(&contents, "<stdin>", vm, symbols, cctx)
}

fn run_file(
    filename: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    let mut contents = fs::read_to_string(filename).map_err(|e| {
        let msg = format!("{}: {}", filename, e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    run_source(&contents, filename, vm, symbols, cctx)
}

/// The agent-first test runner, embedded at build time, in the order its
/// definitions are evaluated. See docs/test-runner.md.
const TEST_RUNNER_FRAGMENTS: &[&str] = &[
    include_str!("test/store.lisp"),
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

/// `elle test ...` — set up a full VM and run the embedded runner with the
/// post-`test` arguments exposed to it as the program argv (via `(sys/argv)`).
/// The runner calls `(os/exit ...)` itself with the gate code; the Ok/Err
/// mapping here is the fallback if it returns without exiting.
fn run_test_subcommand(sub_args: Vec<String>) -> i32 {
    // Split off the global config flags (`--trace=...`, `--stats`,
    // `--no-uring`) so the embedded runner's VM (and the off-VM free-log /
    // page-claim histogram) honour them; the rest become the runner's argv. The
    // runner itself does not interpret these, so without this they would be
    // slurped as corpus file paths. (Runner-owned `--summary`/`--query`/… stay
    // in `sub_args`.) `--no-uring` lets a Linux box run the corpus on the
    // thread-pool backend — the only backend a Mac has — so a pool-only wedge
    // can be chased without a Mac.
    let (config_flags, sub_args): (Vec<String>, Vec<String>) = sub_args
        .into_iter()
        .partition(|a| a.starts_with("--trace=") || a == "--stats" || a == "--no-uring");
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
        match run_source(&test_runner_source(), "src/test", vm, symbols, cctx) {
            Ok(_) => 0,
            Err(_) => 1,
        }
    };
    // The runner usually calls `(os/exit …)` itself (skipping Drop); on a
    // graceful return `rt`'s Drop runs the principled teardown sweep.
    code
}

fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    // --dump=...: run the compiler up to each requested stage, print the
    // artifact, and exit without executing.
    if !elle::config::get().dump.is_empty() {
        return run_dump(contents, source_name, symbols, cctx);
    }

    // WASM backend: compile and run through Wasmtime instead of bytecode VM
    #[cfg(feature = "wasm")]
    if elle::config::get().wasm_full() {
        let eval_fn = if elle::config::get().no_stdlib {
            elle::wasm::eval_wasm
        } else {
            elle::wasm::eval_wasm_with_stdlib
        };
        return match eval_fn(contents, source_name) {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("{}", e);
                Err(e)
            }
        };
    }

    // Compile file as a single letrec
    let result = match compile_file(contents, symbols, cctx, source_name) {
        Ok(r) => r,
        Err(e) => {
            let lerr = parse_compilation_error(&e);
            if elle::config::get().json {
                eprintln!("{}", format_error_json(&lerr));
            } else {
                eprintln!("{}", lerr.format_with_source());
            }
            return Err(e);
        }
    };

    // Debug: print bytecode if --debug is set
    if elle::config::get().has_trace("bytecode") {
        eprintln!(
            "{}",
            elle::compiler::format_bytecode_with_protos(&result.bytecode)
        );
    }

    match vm.execute_scheduled(&result.bytecode, cctx) {
        Ok(_) => {
            // Script mode is silent except for explicit output (display, etc.)
            Ok(())
        }
        Err(e) => {
            // A loud (gate! …) whose condition is unmet propagates an uncaught
            // :gated signal. That is an intentional SKIP, not a failure — report
            // the reason and exit 0, so gate! is a universal skip mechanism (the
            // same intent the test runner records as status=skip). Any other
            // uncaught error still fails.
            if let Some(reason) = vm.take_gated_exit_reason() {
                eprintln!("SKIP (gated): {}", reason);
                return Ok(());
            }
            eprintln!("{}", format_runtime_error(&e, symbols));
            Err("Errors encountered during execution".to_string())
        }
    }
}

fn run_repl(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> bool {
    match Repl::new() {
        Ok(mut repl) => repl.run(vm, symbols, cctx),
        Err(e) => {
            eprintln!("✗ Failed to initialize readline: {}", e);
            Repl::run_fallback(vm, symbols, cctx)
        }
    }
}

#[cfg(feature = "jit")]
fn print_jit_stats(vm: &mut VM) {
    // Drain pending background compilations so stats are complete.
    vm.drain_jit_pending();
    let compiled = vm.jit_cache.len();
    let rejected = vm.jit_rejections.len();

    eprintln!("JIT stats:");
    eprintln!("  compiled: {}", compiled);
    eprintln!("  rejected: {}", rejected);

    if rejected > 0 {
        // Sort by call count ascending
        let mut entries: Vec<_> = vm.jit_rejections.iter().collect();
        entries.sort_by_key(|(ptr, _)| vm.closure_call_counts.get(ptr).copied().unwrap_or(0));

        for (ptr, info) in &entries {
            let name = info.name.as_deref().unwrap_or("<anon>");
            let calls = vm.closure_call_counts.get(ptr).copied().unwrap_or(0);
            eprintln!("    {:<24} {}  [called {}x]", name, info.reason, calls);
        }
    }
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

    // --help and --version answer before VM init, so they still answer in a
    // tree whose stdlib or plugin is broken — which is when somebody asks.
    //
    // The scan stops where `Config::parse` stops reading flags: at `--`. Past
    // it every argument belongs to the program, so a script carries a --help
    // or a --version of its own the way it already carries a --jit.
    let own_flags = match args.iter().position(|a| a == "--") {
        Some(i) => &args[..i],
        None => &args[..],
    };
    if own_flags.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if own_flags.iter().any(|a| a == "--version") {
        println!("{}", elle::BANNER);
        return;
    }

    let (mut config, remaining_args) =
        elle::config::Config::parse(&args[1..]).unwrap_or_else(|e| {
            eprintln!("elle: {}", e);
            std::process::exit(1);
        });

    // Trap POSIX signals at startup, before any thread spawn. This
    // installs sigaction handlers for TERM/INT/QUIT/HUP (clean exit),
    // TSTP/TTIN/TTOU (raise SIGSTOP), CONT (consume), SIGPIPE (ignore),
    // and pthread_sigmask-blocks the absorb-set (USR1/USR2/CHLD/URG/
    // WINCH/ALRM) on the main thread. See `init_process_signals` in
    // src/io/sigfd.rs and docs/posix-signals.md for the full table.
    //
    // Must run before VM::new() because VM construction may spawn
    // worker threads (currently it doesn't, but the JIT worker spawned
    // later inherits whatever mask the main thread holds at its spawn
    // time). Workers' own `mask_all_signals_on_this_thread()` calls
    // are belt-and-suspenders but are not the primary defence.
    elle::io::init_process_signals();

    let mut had_errors = false;
    let mut files: Vec<String> = Vec::new();
    let mut eval_exprs: Vec<String> = Vec::new();
    let mut read_stdin = false;
    let mut source_arg = String::new();
    let mut user_args: Vec<String> = Vec::new();

    // remaining_args from Config::parse: file args, eval expressions (--eval:...), and user args after --.
    // Separate eval expressions from file args. This runs BEFORE Runtime
    // construction so the main source can select the Unicode generation.
    for (i, arg) in remaining_args.iter().enumerate() {
        if let Some(expr) = arg.strip_prefix("--eval:") {
            eval_exprs.push(expr.to_string());
        } else if arg == "-" && files.is_empty() && eval_exprs.is_empty() {
            read_stdin = true;
            source_arg = "-".to_string();
            user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if arg == "--" {
            user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if files.is_empty() && eval_exprs.is_empty() {
            source_arg = arg.clone();
            files.push(arg.clone());
            // Everything after the first file arg goes to user_args
            user_args = remaining_args[i + 1..].to_vec();
            break;
        }
    }
    if !eval_exprs.is_empty() && files.is_empty() && !read_stdin {
        source_arg = "<eval>".to_string();
    }

    // Resolve the Unicode generation before any VM exists: the main file
    // (or the -e expressions) may declare it, and the CLI flag may select
    // it; the surfaces must agree. Stdin and the REPL select via the flag
    // only. A source that fails to parse here is ignored — the compiler
    // reports the parse error properly later. Literate .md sources are
    // not scanned (their code lives inside markdown).
    let scanned_source = if let Some(f) = files.first() {
        if f.ends_with(".md") {
            None
        } else {
            std::fs::read_to_string(f).ok().map(|src| (src, f.clone()))
        }
    } else if !eval_exprs.is_empty() {
        Some((eval_exprs.join("\n"), "<eval>".to_string()))
    } else {
        None
    };
    if let Some((src, name)) = scanned_source {
        if let Ok(Some(request)) = elle::segment::scan_unicode_request(&src, &name) {
            let declared = elle::segment::Generation::from_request(&request).unwrap_or_else(|e| {
                eprintln!("elle: {}: {}", name, e);
                std::process::exit(1);
            });
            match config.unicode {
                Some(flagged) if flagged != declared => {
                    eprintln!(
                        "elle: --unicode={} conflicts with the {} declaration in {}",
                        flagged.version_string(),
                        declared.version_string(),
                        name
                    );
                    std::process::exit(1);
                }
                _ => config.unicode = Some(declared),
            }
        }
    }
    elle::config::init(config);

    // One runtime drives every entry path (file / eval / stdin / REPL); its
    // Drop (or the explicit `teardown` below) runs the principled, RC-driven
    // teardown sweep (docs/impl/region/rules.md § "Teardown — every region frees").
    // The VM reads the resolved Unicode generation from the global config.
    let mut rt = if elle::config::get().no_stdlib {
        Runtime::without_stdlib()
    } else {
        Runtime::new()
    };
    rt.vm().source_arg = source_arg;
    rt.vm().user_args = user_args;

    if read_stdin {
        let (vm, symbols, cctx) = rt.parts();
        if run_stdin(vm, symbols, cctx).is_err() {
            had_errors = true;
        }
    } else if !eval_exprs.is_empty() {
        for expr in &eval_exprs {
            let (vm, symbols, cctx) = rt.parts();
            if run_source(expr, "<eval>", vm, symbols, cctx).is_err() {
                had_errors = true;
            }
        }
    } else if !files.is_empty() {
        for filename in &files {
            let (vm, symbols, cctx) = rt.parts();
            if run_file(filename, vm, symbols, cctx).is_err() {
                had_errors = true;
            }
        }
    } else {
        let (vm, symbols, cctx) = rt.parts();
        if run_repl(vm, symbols, cctx) {
            had_errors = true;
        }
    }

    let stats = elle::config::get().stats;
    if stats {
        #[cfg(feature = "jit")]
        print_jit_stats(rt.vm());
        let cvc = elle::lir::closure_value_const_count();
        if cvc > 0 {
            eprintln!("[stats] closure-valued ValueConsts serialized: {}", cvc);
        }
    }

    // Graceful exit on every path: run the principled teardown sweep explicitly
    // (so it happens before any `process::exit`, which would skip `rt`'s Drop)
    // and surface its observable result under `--stats`.
    let report = rt.teardown();
    if stats {
        eprintln!(
            "[stats] live regions after teardown: {} \
             (0 = clean; residue names open leaks)",
            report.live_regions
        );
    }

    if !read_stdin && files.is_empty() && eval_exprs.is_empty() {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
