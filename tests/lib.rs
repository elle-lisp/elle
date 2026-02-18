// Main test harness - discovers all tests from subdirectories
mod unittests {
    include!("unittests/mod.rs");
}
mod integration {
    include!("integration/mod.rs");
}
mod vm {
    include!("vm/mod.rs");
}
mod property {
    include!("property/mod.rs");
}
