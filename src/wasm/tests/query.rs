// audited: 2026-09-29
//! A primitive that asks the driving VM a question (`SIG_QUERY`) is answered on the full-module tier.
//!
//! docs/impl/wasm.md
//!
//! `vm/config` and the `compile/*` primitives answer through a query the VM
//! services (`VM::dispatch_query`). The counter-factual: a host that hands the
//! query pair back unanswered unwinds the program with it as an error, so any
//! implementation file that reads `(vm/config …)` fails under `--wasm=full`.

use super::*;

#[test]
fn wasm_full_answers_a_vm_config_query() {
    assert_eq!(eval_with_stdlib("(struct? (vm/config))"), "true");
    assert_eq!(eval_with_stdlib("(vm/config :max-depth)"), "10000000");
}
