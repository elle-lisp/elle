// audited: 2026-10-05
//! The wording of the runtime's signal refusals: a bound's entry check, and the arguments of squelch and attune.
//!
//! docs/signals/inference.md
//!
//! The language suite asserts only the kind of each error. What the error
//! says is this implementation's, so it is pinned here.

/// The message `source` raises with when it runs. Panics if it does not raise.
fn raised(source: &str) -> String {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    match crate::pipeline::eval_all(source, symbols, vm, cctx, "<messages>") {
        Ok(_) => panic!("{source} ran to completion"),
        Err(message) => message,
    }
}

/// Assert that `source` raises with a message holding `wording`.
fn raised_with(source: &str, wording: &str) {
    let message = raised(source);
    assert!(
        message.contains(wording),
        "{source} raised {message:?}, which does not say {wording:?}"
    );
}

/// A function whose parameter is bounded to `||`. Each argument below sits in
/// an array, so the compiler cannot refuse it at the call and the entry check
/// answers instead.
const APS: &str = "(defn aps [h x] (bound h ||) (h x))\n";

#[test]
fn an_entry_check_names_a_native_by_its_name() {
    raised_with(
        &format!("{APS}(def r (aps (get @[length] 0) [1 2])) r"),
        "bound: length may emit {:error} but parameter is bounded to {}",
    );
}

#[test]
fn an_entry_check_refuses_a_polymorphic_closure_for_its_polymorphism() {
    raised_with(
        &format!("{APS}(defn call-it [k] (k))\n(def r (aps (get @[call-it] 0) 42)) r"),
        "bound: closure passes on the signal of its arguments but parameter is \
         bounded to {}",
    );
}

#[test]
fn squelch_and_attune_name_an_argument_of_the_wrong_type() {
    // The closure is checked first, so a call with its arguments swapped
    // names the closure position.
    raised_with(
        "(squelch (fn [] 1) :yield)",
        "squelch: second argument must be a closure, got keyword",
    );
    raised_with(
        "(attune (fn [] 1) :yield)",
        "attune: second argument must be a closure, got keyword",
    );
}
