// Property-based tests harness
mod strategies;
mod coroutines {
    include!("coroutines.rs");
}
mod fibers {
    include!("fibers.rs");
}
mod nanboxing {
    include!("nanboxing.rs");
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
mod ffi {
    include!("ffi.rs");
}
mod path {
    include!("path.rs");
}
