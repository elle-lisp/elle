// audited: 2026-11-19
// A REPL def may call a function a later line defines; two defs that call each other do not compile.
// docs/bindings.md

use super::*;

#[test]
fn forward_reference_across_lines() {
    let (out, _, code) = elle("(defn foo [] (bar))\n(defn bar [] 42)\n(foo)\n");
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 42"), "forward ref result: {}", out);
}

/// Each line compiles alone, and a name exists from its def onward, so a
/// retry of either def still misses the other. The session ends with both
/// unresolved and exits 1.
///
/// The counter-factual is a retry that compiles the waiting defs together as
/// one file: it fails the same way, because a file binds no name before its
/// def. A retry that succeeded would print `⟹ :done` and exit 0.
#[test]
fn mutual_recursion_across_lines_stays_unresolved() {
    let (out, err, code) = elle(
        "(defn ping [n] (if (= n 0) :done (pong (- n 1))))\n\
         (defn pong [n] (if (= n 0) :done (ping (- n 1))))\n\
         (ping 10)\n",
    );
    assert_eq!(code, 1, "stdout: {}\nstderr: {}", out, err);
    assert!(
        !out.contains("⟹ :done"),
        "no retry may bind the pair: {}",
        out
    );
    assert!(
        err.contains("✗ ping: unresolved (pong undefined)")
            && err.contains("✗ pong: unresolved (ping undefined)"),
        "both defs stay unresolved: {}",
        err
    );
}

#[test]
fn a_destructured_letrec_binds_mutual_recursion() {
    let (out, err, code) = elle(
        "(def [ping pong]\n\
           (letrec [ping (fn [n] (if (= n 0) :done (pong (- n 1))))\n\
                    pong (fn [n] (if (= n 0) :done (ping (- n 1))))]\n\
             [ping pong]))\n\
         (ping 10)\n",
    );
    assert_eq!(code, 0, "stdout: {}\nstderr: {}", out, err);
    assert!(out.contains("⟹ :done"), "mutual recursion: {}", out);
}

#[test]
fn forward_reference_same_line() {
    let (out, _, code) = elle("(defn foo [] (bar)) (defn bar [] 42) (foo)\n");
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 42"), "forward ref same line: {}", out);
}

#[test]
fn deferred_then_resolved_by_later_def() {
    // foo references bar which doesn't exist yet; bar is defined later;
    // foo should be resolved and callable.
    let (out, _, code) = elle(
        "(defn foo [x] (bar x))\n\
         (defn bar [x] (* x 10))\n\
         (foo 3)\n",
    );
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 30"), "deferred resolution: {}", out);
}

#[test]
fn chained_forward_references() {
    // foo → bar → baz, all defined in forward order
    let (out, _, code) = elle(
        "(defn foo [] (bar))\n\
         (defn bar [] (baz))\n\
         (defn baz [] 99)\n\
         (foo)\n",
    );
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 99"), "chained forward refs: {}", out);
}
