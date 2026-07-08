//! `--help` / `--dump` keyword rendering.

/// Word-wrap a comma-joined keyword list into lines no wider than `width`,
/// so `--help` renders the live `TRACE_KEYWORDS` set without a hand-kept copy.
fn wrap_keywords(keywords: &[&str], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for (i, kw) in keywords.iter().enumerate() {
        let piece = if i + 1 < keywords.len() {
            format!("{kw},")
        } else {
            kw.to_string()
        };
        if !cur.is_empty() && cur.len() + 1 + piece.len() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&piece);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// One-line description for a `--dump` stage. Every entry of
/// `config::DUMP_KEYWORDS` must have one; a new keyword without a description
/// shows "(undocumented)" in `--help`, a loud nudge to add it here.
fn dump_keyword_desc(kw: &str) -> &'static str {
    match kw {
        "ast" => "parsed syntax forms",
        "hir" => "resolved HIR",
        "fhir" => "functionalized HIR (s-expression)",
        "lir" => "lowered LIR (SSA)",
        "jit" => "JIT eligibility per function",
        "cfg" => "per-function control-flow graph",
        "dfa" => "dataflow / signal inference results",
        "defuse" => "HIR def-use chains, value origin, liveness",
        "regions" => "Tofte-Talpin region inference results",
        "escape" => "escape analysis (cross-region reference edges)",
        "git" => "(reserved for SPIR-V output)",
        _ => "(undocumented)",
    }
}

pub(super) fn print_help() {
    println!("Elle v1.0.0\n");
    println!("Usage: elle [file...] [-- args...]       Run files or start REPL");
    println!("       elle fmt [options] <file...>       Format source files");
    println!("       elle lint [options] <file|dir>... Static analysis");
    println!("       elle lsp                          Start language server");
    println!("       elle rewrite [options] <file...>  Source-to-source rewriting\n");
    println!("Options:");
    println!("  -h, --help            Show this help");
    println!("  -e, --eval EXPR       Evaluate expression");
    println!("  -                     Read from stdin");
    println!("  --dump=KW[,KW,...]    Dump compiler artifacts and exit. Keywords:");
    // Driven by DUMP_KEYWORDS so a new stage can't be added without appearing
    // here (the list had already drifted — `escape` was missing).
    for kw in elle::config::DUMP_KEYWORDS {
        println!(
            "                          {kw:<8}— {}",
            dump_keyword_desc(kw)
        );
    }
    println!("  --dump=all            Dump every stage");
    println!("  --jit=POLICY          JIT policy: off (default), eager, adaptive, or integer N");
    println!("  --mlir=POLICY         MLIR policy: off (default), eager, adaptive, or integer N");
    println!("  --wasm=POLICY         WASM policy: off (default), full, lazy, or integer N");
    println!("  --flip=on|off         Legacy no-op (accepted for backwards compat)");
    println!("  --trace=KW[,KW,...]   Trace subsystems. Keywords:");
    // Generated from the single source of truth so help can never drift from
    // the accepted set (a recurring friction point when both were hand-edited).
    for line in wrap_keywords(elle::config::TRACE_KEYWORDS, 50) {
        println!("                          {line}");
    }
    println!("  --trace=all           Trace everything");
    println!("  --stats               Print statistics at normal program termination");
    println!("  --no-stdlib           Skip loading stdlib (debugging compile_core / prelude)");
    println!("  --no-uring            Linux: disable io_uring; route I/O through the thread pool");
    println!("  --home=DIR            Module resolution root (env: ELLE_HOME)");
    println!("  --path=DIRS           Colon-separated module search path (env: ELLE_PATH)");
    println!("  --cache=DIR           Disk cache directory (env: ELLE_CACHE)");
    println!("  --json                JSON output on stderr\n");
    println!("Syntax:");
    println!("  .lisp             S-expression syntax (default)");
    println!("  .py               Python syntax");
    println!("  .js               JavaScript syntax");
    println!("  .lua              Lua syntax");
    println!("  .md               Literate markdown (```lisp blocks)");
}
