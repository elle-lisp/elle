// Property-based tests harness
mod strategies;
mod coroutines {
    include!("coroutines.rs");
}
mod bugfixes {
    include!("bugfixes.rs");
}
mod fibers {
    include!("fibers.rs");
}
mod macros {
    include!("macros.rs");
}
mod destructuring {
    include!("destructuring.rs");
}
mod nanboxing {
    include!("nanboxing.rs");
}
mod determinism {
    include!("determinism.rs");
}
mod arithmetic {
    include!("arithmetic.rs");
}
mod reader {
    include!("reader.rs");
}
mod effects {
    include!("effects.rs");
}
mod strings {
    include!("strings.rs");
}
mod eval {
    include!("eval.rs");
}
mod convert {
    include!("convert.rs");
}
mod ffi {
    include!("ffi.rs");
}
