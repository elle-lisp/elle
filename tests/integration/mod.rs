// Integration tests harness
mod core {
    include!("core.rs");
}
mod advanced {
    include!("advanced.rs");
}
mod types {
    include!("types.rs");
}
mod macros {
    include!("macros.rs");
}
mod metaprogramming {
    include!("metaprogramming.rs");
}
mod stdlib {
    include!("stdlib.rs");
}
mod stability {
    include!("stability.rs");
}
mod property {
    include!("property.rs");
}
mod exception_handling {
    include!("exception_handling.rs");
}
mod handler_case {
    include!("handler_case.rs");
}
mod ffi_marshaling {
    include!("ffi-marshaling.rs");
}
mod ffi_callbacks {
    include!("ffi-callbacks.rs");
}
mod ffi_struct_marshaling {
    include!("ffi-struct-marshaling.rs");
}
mod ffi_union_marshaling {
    include!("ffi-union-marshaling.rs");
}
mod ffi_custom_handlers {
    include!("ffi-custom-handlers.rs");
}
mod ffi_handler_integration {
    include!("ffi-handler-integration.rs");
}
mod shebang {
    include!("shebang.rs");
}
mod finally_clause {
    include!("finally_clause.rs");
}
mod loops {
    include!("loops.rs");
}
mod exception_filtering {
    include!("exception_filtering.rs");
}
mod optimization {
    include!("optimization.rs");
}
mod closures_and_lambdas {
    include!("closures_and_lambdas.rs");
}
mod mutual_recursion {
    include!("mutual_recursion.rs");
}
mod closure_capture_optimization {
    include!("closure_capture_optimization.rs");
}
mod closure_optimization {
    include!("closure_optimization.rs");
}
mod pattern_matching {
    include!("pattern_matching.rs");
}

mod module_qualified_names {
    include!("module_qualified_names.rs");
}

mod tables_and_structs {
    include!("tables_and_structs.rs");
}

mod scoping {
    include!("scoping.rs");
}

mod compiler_core {
    include!("compiler_core.rs");
}

mod primitives_core {
    include!("primitives_core.rs");
}

mod jit_integration {
    include!("jit_integration.rs");
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

mod deep_tail_recursion {
    include!("deep_tail_recursion.rs");
}
