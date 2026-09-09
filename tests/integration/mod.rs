// audited: 2026-09-09
// Registers every integration test file, which is what makes one run.
//
// tests/AGENTS.md
//
// A file here is not compiled until it is named below, so an unregistered file
// is a test suite that reports success having run nothing.
mod core {
    include!("core.rs");
}
mod error_reporting {
    include!("error_reporting.rs");
}
mod diagnostics {
    include!("diagnostics.rs");
}
mod repl_exit_codes {
    include!("repl_exit_codes.rs");
}
mod repl {
    include!("repl.rs");
}
mod new_pipeline {
    include!("new_pipeline.rs");
}
mod pipeline {
    include!("pipeline.rs");
}
mod pipeline_property {
    include!("pipeline_property.rs");
}
mod thread_transfer {
    include!("thread_transfer.rs");
}
mod signal_enforcement {
    include!("signal_enforcement.rs");
}
mod signal_unsoundness {
    include!("signal_unsoundness.rs");
}
#[cfg(feature = "jit")]
mod jit {
    include!("jit.rs");
}
mod time_property {
    include!("time_property.rs");
}
mod time_elapsed {
    include!("time_elapsed.rs");
}
mod deps {
    include!("deps.rs");
}
mod dispatch {
    include!("dispatch.rs");
}
mod lint {
    include!("lint.rs");
}
mod compliance {
    include!("compliance.rs");
}
mod string {
    include!("string.rs");
}
mod escape {
    include!("escape.rs");
}
mod io {
    include!("io.rs");
}
mod net {
    include!("net.rs");
}
mod file_scope {
    include!("file_scope.rs");
}
mod sys_args {
    include!("sys_args.rs");
}
mod meta {
    include!("meta.rs");
}
mod elle_scripts {
    include!("elle_scripts.rs");
}
mod dump_cli {
    include!("dump_cli.rs");
}
mod trace_compile {
    include!("trace_compile.rs");
}
mod flip_cli {
    include!("flip_cli.rs");
}
mod embedding {
    include!("embedding.rs");
}
mod projection {
    include!("projection.rs");
}
mod lsp {
    include!("lsp.rs");
}
mod version {
    include!("version.rs");
}
mod scratch {
    include!("scratch.rs");
}
mod truncation {
    include!("truncation.rs");
}
mod census {
    include!("census.rs");
}
mod image {
    include!("image.rs");
}
mod syntax_regions {
    include!("syntax_regions.rs");
}
mod timeout_capture {
    include!("timeout_capture.rs");
}
mod trace_boot {
    include!("trace_boot.rs");
}
mod trace_isolation {
    include!("trace_isolation.rs");
}
mod trace_residue {
    include!("trace_residue.rs");
}
mod spawn_stack {
    include!("spawn_stack.rs");
}
mod ffi_worker {
    include!("ffi_worker.rs");
}
mod runner_exit_trap {
    include!("runner_exit_trap.rs");
}
#[cfg(target_os = "linux")]
mod subprocess_sigmask {
    include!("subprocess_sigmask.rs");
}
mod unicode_generation {
    include!("unicode_generation.rs");
}
mod paths {
    include!("paths.rs");
}
mod agents {
    include!("agents.rs");
}
mod audit {
    include!("audit.rs");
}
mod prose {
    include!("prose.rs");
}
mod rustsource {
    include!("rustsource.rs");
}
mod profiles {
    include!("profiles.rs");
}
mod bytecode_doc {
    include!("bytecode_doc.rs");
}
mod workflows {
    include!("workflows.rs");
}
mod plugins {
    include!("plugins.rs");
}
mod budget {
    include!("budget.rs");
}
mod capacity {
    include!("capacity.rs");
}
mod doctest {
    include!("doctest.rs");
}

// `allocator.rs` is absent from the list above and does not compile. It calls
// FiberHeap methods the region-ownership model retired — `alloc`, `mark`,
// `release`, `push_scope_mark`, `pop_scope_mark_and_release`,
// `alloc_region_slice` — against a heap that now allocates and refcounts per
// region. The allocator interception it covered is reached by
// `push_custom_allocator`.
