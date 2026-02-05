// Integration tests harness
mod core {
    include!("core.rs");
}
mod advanced {
    include!("advanced.rs");
}
mod stdlib {
    include!("stdlib.rs");
}
mod optimization {
    include!("optimization.rs");
}
mod metaprogramming {
    include!("metaprogramming.rs");
}
mod stability {
    include!("stability.rs");
}
mod macros {
    include!("macros.rs");
}
mod types {
    include!("types.rs");
}
mod property {
    include!("property.rs");
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
mod finally_clause {
    include!("finally_clause.rs");
}
mod loops {
    include!("loops.rs");
}
mod exception_filtering {
    include!("exception_filtering.rs");
}
