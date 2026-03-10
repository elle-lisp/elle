// Integration tests harness
mod core {
    include!("core.rs");
}
// concurrency tests migrated to tests/elle/concurrency.lisp
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
// pipeline_point tests migrated to tests/elle/pipeline.lisp
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
// fibers tests migrated to tests/elle/fibers.lisp
mod time_property {
    include!("time_property.rs");
}
mod time_elapsed {
    include!("time_elapsed.rs");
}
// destructuring tests migrated to tests/elle/destructuring.lisp
mod blocks {
    include!("blocks.rs");
}
// primitives tests migrated to tests/elle/primitives.lisp
// ffi tests migrated to tests/elle/ffi.lisp
// bracket_errors tests migrated to tests/elle/brackets.lisp
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
// splice tests migrated to tests/elle/splice.lisp
// bytes/crypto tests migrated to tests/elle/bytes.lisp and tests/elle/plugins/crypto.lisp
// regex tests migrated to tests/elle/plugins/regex.lisp
// glob tests migrated to tests/elle/plugins/glob.lisp
// fn_flow tests migrated to tests/elle/fn-flow.lisp
// fn_graph tests migrated to tests/elle/fn-graph.lisp
// table_keys tests migrated to tests/elle/table-keys.lisp
// arena tests removed (arena semantics tested in Elle)
mod escape {
    include!("escape.rs");
}
mod allocator {
    include!("allocator.rs");
}
// parameters tests migrated to tests/elle/parameters.lisp
// ports tests migrated to tests/elle/ports.lisp
mod io {
    include!("io.rs");
}
// jit_yield tests migrated to tests/elle/jit-yield.lisp
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
