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
// pipeline_point tests migrated to tests/elle/pipeline.lisp
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
// fibers tests migrated to tests/elle/fibers.lisp
mod time_property {
    include!("time_property.rs");
}
mod time_elapsed {
    include!("time_elapsed.rs");
}
// destructuring tests migrated to tests/elle/destructuring.lisp
// blocks tests migrated to tests/elle/blocks.lisp
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
// FIXME(family-e): `escape.rs` references `LirFunction::rotation_safe`,
// a field retired by the s11 escape-analysis overhaul. Three tests
// (`rotation_safe_for_pure_recursive_functions`,
// `rotation_unsafe_for_push_in_body`,
// `rotation_safe_for_mutual_recursion`) need to either be
// re-derived from the current escape state or replaced with
// equivalent end-to-end checks. Re-enable when fixed.
// See notes.md § "Current failing tests — Family E".
// mod escape {
//     include!("escape.rs");
// }

// FIXME(family-e): `allocator.rs` uses retired FiberHeap APIs
// (alloc, mark, release, push_scope_mark,
// pop_scope_mark_and_release, alloc_inline_slice). The new API
// surface is alloc_in_region / alloc_inline_slice_in_region with
// per-region refcount lifecycle. The whole file needs rewriting
// against that interface — or retired with a replacement Elle-level
// test (the allocator-interception property is exercised by
// push_custom_allocator). Re-enable when rewritten.
// See notes.md § "Current failing tests — Family E".
// mod allocator {
//     include!("allocator.rs");
// }

mod anf_counterfactual {
    include!("anf_counterfactual.rs");
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
// traits tests migrated to tests/elle/traits.lisp
mod sys_args {
    include!("sys_args.rs");
}
// cond_match_args tests migrated to tests/elle/cond-match-args.lisp
mod meta {
    include!("meta.rs");
}
mod elle_scripts {
    include!("elle_scripts.rs");
}
mod dump_cli {
    include!("dump_cli.rs");
}
mod flip_cli {
    include!("flip_cli.rs");
}
// mutability tests migrated to tests/elle/mutability.lisp
mod embedding {
    include!("embedding.rs");
}
mod projection {
    include!("projection.rs");
}
mod lsp {
    include!("lsp.rs");
}

// Temporarily disabled while sorting out compilation caching.
// mod fn_flow {
//     include!("fn_flow.rs");
// }
// mod fn_graph {
//     include!("fn_graph.rs");
// }
