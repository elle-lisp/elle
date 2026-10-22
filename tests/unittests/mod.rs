// audited: 2026-10-22
// The unit-test harness: one module per file in this directory, each pulled
// into the shared test binary.
//
// tests/AGENTS.md
mod symbol {
    include!("symbol.rs");
}
mod value {
    include!("value.rs");
}
mod primitives {
    include!("primitives.rs");
}
mod closures_and_lambdas {
    include!("closures_and_lambdas.rs");
}
mod bytecode_debug {
    include!("bytecode_debug.rs");
}
mod bytecode_safety {
    include!("bytecode_safety.rs");
}
mod hir_debug {
    include!("hir_debug.rs");
}
#[cfg(feature = "jit")]
mod jit {
    include!("jit.rs");
}
mod table_key {
    include!("table_key.rs");
}
mod cycle {
    include!("cycle.rs");
}
mod mutable_store {
    include!("mutable_store.rs");
}
