//! Smoke test: compile Elle source through LIR → WASM → Wasmtime.
#![cfg(feature = "wasm")]

fn eval(source: &str) -> String {
    // Run the WASM backend the way `elle --wasm=full` does, with no shared
    // disk cache so each test compiles its module independently and
    // deterministically.
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        elle::config::init(elle::config::Config {
            cache: None,
            ..Default::default()
        });
    });
    match elle::wasm::eval_wasm_with_stdlib(source, "<test>") {
        Ok(result) => result,
        Err(e) => panic!("eval_wasm('{}') failed: {}", source, e),
    }
}

#[path = "wasm_smoke/core.rs"]
mod core;
#[path = "wasm_smoke/fibers.rs"]
mod fibers;
