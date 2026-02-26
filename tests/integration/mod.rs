// Integration tests harness
mod core {
    include!("core.rs");
}
mod advanced {
    include!("advanced.rs");
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
mod coroutines {
    include!("coroutines.rs");
}
mod lexical_scope {
    include!("lexical_scope.rs");
}
mod new_pipeline {
    include!("new_pipeline.rs");
}
mod new_pipeline_property {
    include!("new_pipeline_property.rs");
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
mod hygiene {
    include!("hygiene.rs");
}
mod destructuring {
    include!("destructuring.rs");
}
mod prelude {
    include!("prelude.rs");
}
mod blocks {
    include!("blocks.rs");
}
mod eval {
    include!("eval.rs");
}
mod primitives {
    include!("primitives.rs");
}
mod ffi {
    include!("ffi.rs");
}
mod bracket_errors {
    include!("bracket_errors.rs");
}
mod booleans {
    include!("booleans.rs");
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
