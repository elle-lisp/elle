(elle/epoch 9)
## Regression tests for JIT compilation behavior.
## Covers: side-exit through yielding primitives called from hot (JIT-compiled) functions.


# ============================================================================
# Regression: JIT side-exit panic when port/write is called from a hot defn.
# After ~10 calls, `ewrite` gets JIT-compiled. The 10th call triggers a side-exit
# through port/write (SIG_IO|SIG_YIELD). This panicked at call.rs:221 before the fix.
# ============================================================================

(def out (port/stdout))
(defn ewrite [s]
  (port/write out s))
(def @jit-lines @[])
(def @i 0)
(while (< i 10)
  (push jit-lines (string/join ["line " (string i)] ""))
  (ewrite (string/join ["line " (string i) "\n"] ""))
  (assign i (+ i 1)))
(assert (= (length jit-lines) 10)
  "jit: port/write in hot defn completes all 10 iterations")
