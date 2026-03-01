// Unit tests harness
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
mod hir_debug {
    include!("hir_debug.rs");
}
mod lir_debug {
    include!("lir_debug.rs");
}
mod jit {
    include!("jit.rs");
}
mod table_key {
    include!("table_key.rs");
}
