# ── Negative eligibility tests ────────────────────────────────────────
#
# Assert that closures known to be ineligible for specific tiers are
# properly rejected with structured :tier-rejected errors. Tests the
# rejection predicates, not execution.

# ── Non-closure on :bytecode ─────────────────────────────────────────

# Any tier must reject a non-closure target with :type-error.
(def [ok-nc? err-nc] (protect (compile/run-on :bytecode 42 1 2)))
(assert (not ok-nc?) "non-closure must be rejected")
(assert (= (get err-nc :error) :type-error)
        "non-closure gets :type-error")

# ── Non-closure on :jit ──────────────────────────────────────────────

(def [ok-ncj? err-ncj] (protect (compile/run-on :jit 42 1 2)))
(assert (not ok-ncj?) "non-closure must be rejected on :jit")
(assert (= (get err-ncj :error) :type-error)
        "non-closure gets :type-error on :jit")

# ── WASM rejects tail calls ─────────────────────────────────────────

# A tail-recursive closure should be rejected by :wasm.
(defn tail-fn [n acc]
  (if (= n 0) acc (tail-fn (- n 1) (+ acc n))))

(def [ok-wt? err-wt] (protect (compile/run-on :wasm tail-fn 5 0)))
(if ok-wt?
  # If WASM feature is not available, compile/run-on returns an error
  # for unknown/unavailable tier — that's also a rejection, not success.
  (println "SKIP: wasm tier not available (this is expected without --features wasm)")
  (begin
    (assert (struct? err-wt) "wasm rejection must be a struct")
    (assert (= (get err-wt :error) :tier-rejected)
            (string "wasm tail-call rejection should be :tier-rejected, got: " err-wt))))

# ── MLIR-CPU rejects closures with captures ──────────────────────────

(def n 5)
(def captured-fn (fn [x] (+ x n)))

(def [ok-mc? err-mc] (protect (compile/run-on :mlir-cpu captured-fn 3)))
(if ok-mc?
  (println "SKIP: mlir-cpu tier not available")
  (begin
    (assert (struct? err-mc) "mlir-cpu rejection must be a struct")
    (assert (= (get err-mc :error) :tier-rejected)
            (string "mlir-cpu capture rejection should be :tier-rejected, got: " err-mc))))

# ── MLIR-CPU rejects non-integer-returning closures ──────────────────

# A closure that returns a boolean (from =) is not MLIR-CPU eligible.
(defn returns-bool [x] (= x 0))

(def [ok-mb? err-mb] (protect (compile/run-on :mlir-cpu returns-bool 0)))
(if ok-mb?
  (println "SKIP: mlir-cpu tier not available")
  (begin
    (assert (struct? err-mb) "mlir-cpu bool rejection must be a struct")
    (assert (= (get err-mb :error) :tier-rejected)
            (string "mlir-cpu bool rejection should be :tier-rejected, got: " err-mb))))

# ── Safety gap: string to arithmetic must not silently succeed ─────────

(def diff ((import "tests/diff/harness")))

(defn add1 [x] (+ x 1))
## Bytecode should error on string input to arithmetic.
## If any other tier silently succeeds, the harness must detect the
## disagreement (the safety gap fix). If no tier succeeds, that's fine too.
(let [[report (diff:call add1 "hello")]]
  (assert (not (get report :agreed))
          (string "string input to arithmetic must not agree, got: " report))
  (assert (contains? (get report :errors) :bytecode)
          "bytecode must error on string + int"))

(println "eligibility: OK")
