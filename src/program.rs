// audited: 2026-10-06
//! What `elle` and the rig share: the run path of one `Runtime`, from a file, `-e`, stdin or the REPL, and the subcommands.
//!
//! docs/config.md
//! rig/overview.md
//!
//! Both executables parse their own command line and hand the resulting
//! `Config` and the program's arguments to [`run`]. Everything from there —
//! the Unicode prescan, the runtime, the gated-exit line, the error report,
//! the statistics and the teardown — is this module's, so a program that runs
//! under `elle` runs under the rig unchanged.

use crate::config::Config;
use crate::pipeline::{compile_file, CompileCtx};
use crate::repl::Repl;
use crate::runtime::Runtime;
use crate::{SymbolTable, VM};
use std::io::Read;

mod dump;
mod errors;
mod image;
mod semver;
mod subcommand;
use errors::{format_error_json, format_runtime_error, parse_compilation_error};
pub use subcommand::subcommand;

/// Run a program as `elle` does, and answer the process exit code.
///
/// `remaining_args` is what `Config::parse` handed back: the `-e` expressions
/// as `--eval:EXPR` entries, then the program name and its arguments. With no
/// source at all this starts the REPL. `config` is installed as the process
/// configuration here, once the main source has had its say about the Unicode
/// generation.
pub fn run(mut config: Config, remaining_args: Vec<String>) -> i32 {
    // Trap POSIX signals at startup, before any thread spawn. This
    // installs sigaction handlers for TERM/INT/QUIT/HUP (clean exit),
    // TSTP/TTIN/TTOU (raise SIGSTOP), CONT (consume), SIGPIPE (ignore),
    // and pthread_sigmask-blocks the absorb-set (USR1/USR2/CHLD/URG/
    // WINCH/ALRM) on the main thread. See `init_process_signals` in
    // src/io/sigfd.rs and docs/posix-signals.md for the full table.
    //
    // Must run before the runtime is built: the JIT worker spawned later
    // inherits whatever mask the main thread holds at its spawn time.
    crate::io::init_process_signals();

    let mut had_errors = false;
    let mut files: Vec<String> = Vec::new();
    let mut eval_exprs: Vec<String> = Vec::new();
    let mut read_stdin = false;
    let mut source_arg = String::new();
    let mut user_args: Vec<String> = Vec::new();

    // Separate the eval expressions from the program name. This runs before
    // the runtime is built so the main source can select the Unicode
    // generation.
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
            // Everything after the program name is the program's.
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
        if let Ok(Some(request)) = crate::segment::scan_unicode_request(&src, &name) {
            let declared = match crate::segment::Generation::from_request(&request) {
                Ok(gen) => gen,
                Err(e) => {
                    eprintln!("elle: {}: {}", name, e);
                    return 1;
                }
            };
            match config.unicode {
                Some(flagged) if flagged != declared => {
                    eprintln!(
                        "elle: --unicode={} conflicts with the {} declaration in {}",
                        flagged.version_string(),
                        declared.version_string(),
                        name
                    );
                    return 1;
                }
                _ => config.unicode = Some(declared),
            }
        }
    }
    crate::config::init(config);

    // One runtime drives every entry path (file / eval / stdin / REPL); its
    // Drop (or the explicit `teardown` below) runs the principled, RC-driven
    // teardown sweep (docs/impl/region/rules.md). The VM reads the resolved
    // Unicode generation from the global config.
    let mut rt = if crate::config::get().no_stdlib {
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

    let stats = crate::config::get().stats;
    if stats {
        #[cfg(feature = "jit")]
        print_jit_stats(rt.vm());
        let cvc = crate::lir::closure_value_const_count();
        if cvc > 0 {
            eprintln!("[stats] closure-valued ValueConsts serialized: {}", cvc);
        }
    }

    // Graceful exit on every path: run the principled teardown sweep explicitly
    // (so it happens before any `process::exit`, which would skip `rt`'s Drop)
    // and surface its observable result under `--dump=stats`.
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

    i32::from(had_errors)
}

/// Run the program on stdin.
pub fn run_stdin(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    let mut contents = String::new();
    std::io::stdin()
        .read_to_string(&mut contents)
        .map_err(|e| {
            let msg = format!("Failed to read stdin: {}", e);
            eprintln!("✗ {}", msg);
            msg
        })?;
    run_source(&contents, "<stdin>", vm, symbols, cctx)
}

/// Run the program in `filename`, less a leading shebang line.
pub fn run_file(
    filename: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    let mut contents = std::fs::read_to_string(filename).map_err(|e| {
        let msg = format!("{}: {}", filename, e);
        eprintln!("✗ {}", msg);
        msg
    })?;
    // Strip a shebang line (for example, #!/usr/bin/env elle).
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    run_source(&contents, filename, vm, symbols, cctx)
}

/// Compile and run `contents` as one file, and report how it ended.
///
/// Under `--dump=`, run the compiler up to each requested stage, print the
/// artifact, and run nothing. An uncaught `:gated` error is a SKIP: it prints
/// `SKIP (gated): REASON` and answers `Ok`.
pub fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    if !crate::config::get().dump.is_empty() {
        return dump::run_dump(contents, source_name, symbols, cctx);
    }

    // WASM backend: compile and run through Wasmtime instead of bytecode VM
    #[cfg(feature = "wasm")]
    if crate::config::get().wasm_full() {
        let eval_fn = if crate::config::get().no_stdlib {
            crate::wasm::eval_wasm
        } else {
            crate::wasm::eval_wasm_with_stdlib
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
            if crate::config::get().json {
                eprintln!("{}", format_error_json(&lerr));
            } else {
                eprintln!("{}", lerr.format_with_source());
            }
            return Err(e);
        }
    };

    if crate::config::get().has_trace("bytecode") {
        eprintln!(
            "{}",
            crate::compiler::format_bytecode_with_protos(&result.bytecode)
        );
    }

    match vm.execute_scheduled(&result.bytecode, cctx) {
        Ok(value) => {
            // The run hands its last form's value over with one owning
            // reference, and nothing here reads the value again: script mode is
            // silent except for the explicit output the program itself wrote.
            // Give the reference back (docs/impl/region/rules.md).
            crate::value::arena::release_program_value(vm.heap(), value);
            Ok(())
        }
        Err(e) => {
            // A loud (gate! …) whose condition is unmet propagates an uncaught
            // :gated signal. That is an intentional SKIP, not a failure — report
            // the reason and exit 0, so gate! is a universal skip mechanism (the
            // same intent the test runner records as status=skip). Any other
            // uncaught error still fails.
            //
            // This line is read, not only printed: `elle test --isolate` runs a
            // file as a child and has nothing but its exit status to judge by,
            // and exit 0 alone would record a gated file as a vacuous pass. The
            // runner matches this prefix on the child's stderr to recover the
            // skip and its reason (src/test/child.lisp `gated-marker`), so the
            // text is a contract between the two.
            if let Some(reason) = vm.take_gated_exit_reason() {
                eprintln!("SKIP (gated): {}", reason);
                return Ok(());
            }
            eprintln!("{}", format_runtime_error(&e, symbols));
            Err("Errors encountered during execution".to_string())
        }
    }
}

/// Run the REPL, and answer whether any form in it failed.
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
