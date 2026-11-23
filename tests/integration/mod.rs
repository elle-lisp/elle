// audited: 2026-11-23
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
mod bins {
    include!("bins.rs");
}
mod macro_purity {
    include!("macro_purity.rs");
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
mod perf_target {
    include!("perf_target.rs");
}
mod embedding {
    include!("embedding.rs");
}
mod squelch {
    include!("squelch.rs");
}
mod scratch {
    include!("scratch.rs");
}
mod forks {
    include!("forks.rs");
}
mod run_artifacts {
    include!("run_artifacts.rs");
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
mod unicode_generation {
    include!("unicode_generation.rs");
}
mod paths {
    include!("paths.rs");
}
mod demos {
    include!("demos.rs");
}
mod audit {
    include!("audit.rs");
}
mod audit_report {
    include!("audit_report.rs");
}
mod audit_counts {
    include!("audit_counts.rs");
}
mod audit_policy {
    include!("audit_policy.rs");
}
mod audit_sources {
    include!("audit_sources.rs");
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
mod db_flag_doc {
    include!("db_flag_doc.rs");
}
mod workflows {
    include!("workflows.rs");
}
mod ci_profile {
    include!("ci_profile.rs");
}
mod ci_steps {
    include!("ci_steps.rs");
}
mod smoke_jobs {
    include!("smoke_jobs.rs");
}
mod qa_gate {
    include!("qa_gate.rs");
}
mod crosscheck {
    include!("crosscheck.rs");
}
mod change_filter {
    include!("change_filter.rs");
}
mod plugins {
    include!("plugins.rs");
}
mod suite_plugins {
    include!("suite_plugins.rs");
}
mod corpus_targets {
    include!("corpus_targets.rs");
}
mod suites {
    include!("suites.rs");
}
mod variants {
    include!("variants.rs");
}
mod capacity {
    include!("capacity.rs");
}
mod blob_build {
    include!("blob_build.rs");
}
mod doctest {
    include!("doctest.rs");
}
mod doctest_scope {
    include!("doctest_scope.rs");
}
mod joined_comments {
    include!("joined_comments.rs");
}
mod test_setup {
    include!("test_setup.rs");
}
mod toolchains {
    include!("toolchains.rs");
}
mod ledgers {
    include!("ledgers.rs");
}
mod benches {
    include!("benches.rs");
}

// `allocator.rs` is absent from the list above and does not compile. It calls
// FiberHeap methods the region-ownership model retired — `alloc`, `mark`,
// `release`, `push_scope_mark`, `pop_scope_mark_and_release`,
// `alloc_region_slice` — against a heap that now allocates and refcounts per
// region. The allocator interception it covered is reached by
// `push_custom_allocator`.
