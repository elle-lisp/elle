// audited: 2026-10-22
//! The wording of the analyzer's signal refusals: a bound, a ceiling, a bad spec, a wrong count.
//!
//! docs/signals/inference.md
//!
//! The language suite asserts only that these programs do not compile. What
//! the refusal says is this implementation's, so it is pinned here.

/// The message `source` fails to compile with. Panics if it compiles.
fn refusal(source: &str) -> String {
    let mut rt = crate::runtime::Runtime::new();
    let (_, symbols, cctx) = rt.parts();
    match crate::pipeline::compile_file(source, symbols, cctx, "<messages>") {
        Ok(_) => panic!("{source} compiled"),
        Err(message) => message,
    }
}

/// Assert that `source` fails to compile with a message holding `wording`.
fn refused_with(source: &str, wording: &str) {
    let message = refusal(source);
    assert!(
        message.contains(wording),
        "{source} was refused with {message:?}, which does not say {wording:?}"
    );
}

#[test]
fn a_malformed_bound_names_the_form_and_its_fault() {
    refused_with("(bound g ||)", "bound must appear inside a function body");
    refused_with(
        "(defn f [x] (bound z ||) x)",
        "bound: 'z' is not a parameter of this function",
    );
    refused_with(
        "(defn f [h] (bound h ||) (bound h :yield) h)",
        "bound: parameter h is already bounded",
    );
    refused_with(
        "(defn f [h] (bound h :nosuch) h)",
        "bound: unknown signal keyword :nosuch",
    );
    refused_with(
        "(defn f [h] (bound h) h)",
        "bound requires a parameter name and a signal spec",
    );
}

#[test]
fn a_bound_refused_at_the_call_names_the_excess_and_the_bound() {
    let aps = "(defn aps [h x] (bound h ||) (h x))\n";
    refused_with(
        &format!("{aps}(aps (fn [x] (yield x)) 1)"),
        "bound: argument may emit {:yield} but parameter h is bounded to {}",
    );
    refused_with(
        &format!("{aps}(aps length [1 2])"),
        "bound: argument may emit {:error} but parameter h is bounded to {}",
    );
    refused_with(
        &format!("{aps}(defn call-it [k] (k))\n(aps call-it 1)"),
        "bound: argument passes on the signal of its arguments but parameter h is \
         bounded to {}",
    );
    // The excess is what the bound does not admit, in the registry's spelling.
    // Reading the standard port is ambient, and writing to it is I/O.
    refused_with(
        "(defn each-yield [h x] (bound h :yield) (h x))\n\
         (each-yield (fn [x] (port/write (port/stdout) x)) 1)",
        "bound: argument may emit {:error, :io, :ambient} but parameter h is bounded to \
         {:yield}",
    );
}

#[test]
fn a_ceiling_names_what_the_body_may_emit() {
    let wording = "function restricted to {} but body may emit {:error}";
    refused_with("(defn f [x y] (silent!) (+ x y))", wording);
    // One ceiling, so one message: silent! is (attune! ||) by another name.
    refused_with("(defn f [x y] (attune! ||) (+ x y))", wording);
    refused_with(
        "(defn f [x y] (silent!) (attune! :error) (+ x y))",
        "body may emit {:error}",
    );
}

#[test]
fn a_bad_spec_names_the_form_that_holds_it() {
    // Counter-factual: the message began with emit:, a form the program never
    // wrote.
    refused_with(
        "(defn f [] (attune! :nosuch) 1)",
        "attune!: unknown signal keyword :nosuch",
    );
}

#[test]
fn a_wrong_count_names_the_callee_and_both_counts() {
    refused_with(
        "(defn g [] (identity 1 2))",
        "arity error: identity expects 1 argument, got 2",
    );
}
