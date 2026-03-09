// Integration tests harness
mod core {
    include!("core.rs");
}
mod concurrency {
    include!("concurrency.rs");
}
mod error_reporting {
    include!("error_reporting.rs");
}
mod repl_exit_codes {
    include!("repl_exit_codes.rs");
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
mod pipeline_point {
    include!("pipeline_point.rs");
}
mod thread_transfer {
    include!("thread_transfer.rs");
}
mod effect_enforcement {
    include!("effect_enforcement.rs");
}
mod effect_unsoundness {
    include!("effect_unsoundness.rs");
}
mod jit {
    include!("jit.rs");
}
mod fibers {
    include!("fibers.rs");
}
mod time_property {
    include!("time_property.rs");
}
mod time_elapsed {
    include!("time_elapsed.rs");
}
mod destructuring {
    include!("destructuring.rs");
}
mod blocks {
    include!("blocks.rs");
}
mod primitives {
    include!("primitives.rs");
}
// ffi tests migrated to tests/elle/ffi.lisp
// Keeping only epsilon tolerance and error message tests in Rust
mod ffi {
    include!("ffi.rs");
}
mod bracket_syntax {
    include!("bracket_errors.rs");
}
mod dispatch {
    include!("dispatch.rs");
}
mod lint {
    include!("lint.rs");
}
mod lsp {
    include!("lsp.rs");
}
mod compliance {
    include!("compliance.rs");
}
mod buffer {
    include!("buffer.rs");
}
// splice tests migrated to tests/elle/splice.lisp
mod bytes {
    include!("bytes.rs");
}
mod regex {
    include!("regex.rs");
}
// glob tests migrated to tests/elle/glob.lisp
// Keeping only plugin availability tests in Rust
mod glob {
    include!("glob.rs");
}
// fn_flow tests migrated to tests/elle/fn-flow.lisp
// fn_graph tests migrated to tests/elle/fn-graph.lisp
mod table_keys {
    include!("table_keys.rs");
}
mod arena {
    include!("arena.rs");
}
mod escape {
    include!("escape.rs");
}
mod allocator {
    include!("allocator.rs");
}
mod parameters {
    include!("parameters.rs");
}
mod ports {
    include!("ports.rs");
}
mod io {
    include!("io.rs");
}
mod jit_yield {
    include!("jit_yield.rs");
}
mod net {
    include!("net.rs");
}
mod file_scope {
    include!("file_scope.rs");
}

// Temporarily disabled while sorting out compilation caching.
// mod fn_flow {
//     include!("fn_flow.rs");
// }
// mod fn_graph {
//     include!("fn_graph.rs");
// }
