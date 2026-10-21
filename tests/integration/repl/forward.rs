// audited: 2026-10-21
// A REPL line may call a function a later line defines, mutual recursion included.
// docs/pipeline.md

use super::*;

#[test]
fn forward_reference_across_lines() {
    let (out, _, code) = elle("(defn foo [] (bar))\n(defn bar [] 42)\n(foo)\n");
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 42"), "forward ref result: {}", out);
}

#[test]
fn mutual_recursion_across_lines() {
    let (out, _, code) = elle(
        "(defn ping [n] (if (= n 0) :done (pong (- n 1))))\n\
         (defn pong [n] (if (= n 0) :done (ping (- n 1))))\n\
         (ping 10)\n",
    );
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ :done"), "mutual recursion: {}", out);
}

#[test]
fn forward_reference_same_line() {
    let (out, _, code) = elle("(defn foo [] (bar)) (defn bar [] 42) (foo)\n");
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ 42"), "forward ref same line: {}", out);
}

#[test]
fn three_way_mutual_recursion() {
    // a(5) → b(4) → c(3) → a(2) → b(1) → c(0) → :c
    let (out, _, code) = elle(
        "(defn a [n] (if (= n 0) :a (b (- n 1))))\n\
         (defn b [n] (if (= n 0) :b (c (- n 1))))\n\
         (defn c [n] (if (= n 0) :c (a (- n 1))))\n\
         (a 5)\n",
    );
    assert_eq!(code, 0, "stdout: {}\n", out);
    assert!(out.contains("⟹ :c"), "three-way mutual recursion: {}", out);
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
