// Integration tests harness
mod core {
    include!("core.rs");
}
mod advanced {
    include!("advanced.rs");
}
mod exception_handling {
    include!("exception_handling.rs");
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
