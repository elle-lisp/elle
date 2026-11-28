// audited: 2026-11-25
// The property tests: the shared strategies, and one module per domain.
//
// tests/property/overview.md

mod strategies;
mod nanboxing {
    include!("nanboxing.rs");
}
mod reader {
    include!("reader.rs");
}
mod signals {
    include!("signals.rs");
}
mod ffi {
    include!("ffi.rs");
}
mod ordering {
    include!("ordering.rs");
}
mod quoting {
    include!("quoting.rs");
}
