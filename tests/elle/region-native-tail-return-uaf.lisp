(elle/epoch 12)
# Counterfactual: a function whose TAIL expression returns the heap result of a
# pass-through native — `(defn f (xs) (first xs))` / `(get xs 0)` / `(xs 0)` —
# hands that result back to its caller WITHOUT a ReturnValue retain, so the
# caller's `DecrefValueRegion` frees it under the caller's own borrow:
# use-after-free.
#
# WITNESSED under `--trace=guardfree` (the run wiring passes that flag). Without
# guardfree the freed page is not yet recycled, so the value reads back intact
# and the loop "passes" — guardfree is the robust oracle, exactly as
# region-jit-passthrough.lisp / region-mutable-reassign-param.lisp.
#
# THE CULPRIT (bisected, LIR-confirmed): a native tail call lowers to
#   tailcall <native>(args)        ; result -> r_res
#   decref-value-region <owned-arg>
#   -> Return(r_res)               ; <-- NO incref-value-region (ReturnValue)
# For an IMMEDIATE result (e.g. `length` -> int) the missing retain is harmless.
# For a HEAP pass-through result (`first`/`rest`/`get`, or a collection
# call-index `(xs i)` — all of which return a value borrowed from another region
# with one pass-through retain from `dispatch_native_call`/`dispatch_collection_call`)
# the function must add the ReturnValue retain before returning, or the single
# owning reference is consumed by the caller's `DecrefValueRegion` and the value
# is freed while still live. The native-tail Inc4 fall-through
# (`tail_call_inner`, src/vm/call.rs) runs the compiler's post-`TailCall` block,
# whose `Return` omits the heap-result retain.
#
# NOT a call_collection defect: `first` and `get` fault IDENTICALLY to the
# call-index `(xs i)` (this file proves it), so the fix belongs to the
# native-tail return convention, not to `call_collection`. docs/impl/region-rules.md
# Rules 4, 5, 8.
#
# RED now (under guardfree): the three tail-return subjects SIGSEGV. GREEN once
# the native-tail post-block retains a heap result before `Return`. The controls
# — the SAME accessor whose result is CONSUMED by a borrowing native (`length`)
# rather than tail-returned — are correct NOW.

# This same defect is what the dns/parse-resolv-conf shape trips: a function
# whose tail expression is `(freeze (filter … …))` / `(map … …)` tail-returns a
# stdlib HOF's heap call-result, so it faults here too — `ret_map` below is that
# manifestation, NOT a separate "filter" bug (consuming the HOF result with a
# borrowing native is correct; only the tail-return faults).

# ── controls: result consumed by a borrowing native, NOT tail-returned ─────────
# These exercise the same `first`/`get`/call-index accessors but feed the heap
# result to `length` (borrows, returns an immediate). Correct NOW — the bug is
# specifically the tail-RETURN of the heap value, not the access.
#
# Loop sizing: hundreds of iterations is far past the adaptive-JIT threshold
# (10 calls) while keeping the whole file inside the guardfree mapping budget
# (vm.max_map_count): the oracle leaks one PROT_NONE mapping per FREED region
# page, so a reclaiming stdlib-heavy loop consumes mappings per iteration and
# an oversized count aborts on mmap exhaustion, not a UAF.

(defn ctl_first (xs)
  (length (first xs)))
(defn ctl_get (xs)
  (length (get xs 0)))
(defn ctl_call (xs)
  (length (xs 0)))

# ── subjects: tail-return the heap pass-through result ─────────────────────────
(defn ret_first (xs)
  (first xs))
(defn ret_get (xs)
  (get xs 0))
(defn ret_call (xs)
  (xs 0))

# ── controls run first (must stay correct) ─────────────────────────────────────
(var c 0)
(var rc1 0)
(while (%lt c 500)
  (assign rc1 (ctl_first (list (concat "xy" "z"))))
  (assign rc1 (ctl_get [(concat "xy" "z")]))
  (assign rc1 (ctl_call [(concat "xy" "z")]))
  (assign c (%add c 1)))
(assert (= rc1 3)
        "control: borrowing-consumer accessor mis-read (harness broken)")

# ── witnesses: each tail-returned heap result must survive the caller's borrow ──
(var i 0)
(var a "")
(var b "")
(var d "")
(while (%lt i 500)
  (assign a (ret_first (list (concat "xy" "z"))))
  (assign b (ret_get [(concat "xy" "z")]))
  (assign d (ret_call [(concat "xy" "z")]))
  (assign i (%add i 1)))
(assert (= a "xyz")
        "(first xs) tail-returned: result freed under the caller's borrow")
(assert (= b "xyz")
        "(get xs 0) tail-returned: result freed under the caller's borrow")
(assert (= d "xyz")
        "(xs 0) call-index tail-returned: result freed under the caller's borrow")

(println "region-native-tail-return-uaf: ok")
