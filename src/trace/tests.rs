//! Unit tests (`super` is the parent impl module).

use std::cell::Cell;
use std::fmt;

/// A label whose formatting is observable, so a test can tell whether the
/// macro reached it.
struct Probe<'a>(&'a Cell<bool>);

impl fmt::Display for Probe<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.set(true);
        f.write_str("probe")
    }
}

// The trap the macro exists for: Rust evaluates a call's arguments before the
// call, so a `phase` taking an already-formatted `&str` builds its label
// whether or not tracing is on — six `format!`s per file compiled, every
// compile. A macro can put the formatting inside the branch; a function
// cannot.
//
// The counter-factual: against the function form this test fails, because
// `&format!("{}", Probe(&reached))` runs at the call site.
#[test]
fn a_disabled_phase_never_formats_its_label() {
    let reached = Cell::new(false);
    let start = std::time::Instant::now();
    crate::phase!(false, "test", start, "{}", Probe(&reached));
    assert!(!reached.get(), "a disabled phase formatted its label");
}

// The other half: the label must still reach the output when tracing is on, or
// the branch above would be indistinguishable from a mark that never prints.
#[test]
fn an_enabled_phase_formats_its_label() {
    let reached = Cell::new(false);
    let start = std::time::Instant::now();
    crate::phase!(true, "test", start, "{}", Probe(&reached));
    assert!(reached.get(), "an enabled phase did not format its label");
}
