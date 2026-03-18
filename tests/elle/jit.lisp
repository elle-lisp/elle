## Regression tests for JIT compilation behavior.
## Covers: side-exit through yielding primitives called from hot (JIT-compiled) functions.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Regression: JIT side-exit panic when stream/write is called from a hot defn.
# After ~10 calls, `ewrite` gets JIT-compiled. The 10th call triggers a side-exit
# through stream/write (SIG_IO|SIG_YIELD). This panicked at call.rs:221 before the fix.
# ============================================================================

(def out (port/stdout))
(defn ewrite [s]
  (stream/write out s))
(var jit-lines @[])
(var i 0)
(while (< i 10)
  (push jit-lines (string/join ["line " (string i)] ""))
  (ewrite (string/join ["line " (string i) "\n"] ""))
  (assign i (+ i 1)))
(assert-eq (length jit-lines) 10 "jit: stream/write in hot defn completes all 10 iterations")
