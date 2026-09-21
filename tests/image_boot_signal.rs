// audited: 2026-09-21
//! A closure carrying a user-declared signal's bit refuses the dump, naming
//! the signal.
//!
//! docs/impl/image/sealing.md
//!
//! This is its own test binary because the signal registry is process-global.
//! A declaration here hands out a bit that every later test in the same binary
//! would see, and the shared integration binary runs its tests in parallel.

use elle::image::{self, ImageError};
use elle::pipeline::eval_all;
use elle::runtime::Runtime;

/// A function that emits a user-declared signal, so its inferred signal
/// carries a bit the registry handed out in this process alone.
const EMITTER: &str = "(signal :image_probe_sig)\n\
                       (defn probe [] (emit :image_probe_sig {:p 1}))\n\
                       probe";

// A user signal's bit is minted in declaration order by a process-global
// registry, and no image carries a signal table (docs/impl/image/format.md
// lists it among the sections still to land). A closure whose signal names one
// would hydrate claiming a bit that means a different signal — or nothing at
// all — in the instance that loads it.
//
// The counter-factual is a check on the registry rather than on the graph: it
// refuses every dump any process that ever declared a signal attempts,
// including the boot dumps of programs whose own graphs name no user bit.
#[test]
fn a_closure_naming_a_user_signal_refuses_the_dump() {
    let dir = std::env::temp_dir().join(format!("elle-boot-signal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("signal.image");

    let mut rt = Runtime::new();
    let probe = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(EMITTER, symbols, vm, cctx, "<image-signal>").expect("eval")
    };
    assert!(
        probe
            .as_closure()
            .is_some_and(|c| c.template.signal().bits.raw() >> 32 != 0),
        "the probe carries no user signal bit, so this test exercises nothing"
    );

    let (heap, symbols) = rt.heap_and_symbols();
    match image::dump(heap, symbols, probe, &path) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains("image_probe_sig"),
            "the refusal does not name the signal: {what}"
        ),
        other => panic!("expected a refused dump, got {other:?}"),
    }
    assert!(!path.exists(), "a refused dump left a partial file");
    let _ = std::fs::remove_dir_all(&dir);
}
