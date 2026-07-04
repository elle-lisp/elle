(elle/epoch 12)
# Correctness guard: a function whose TAIL expression is a SPLICE/`apply` call
# to a heap pass-through native — `(first ;argv)` / `(get ;argv)` / a spliced
# call-index — must hand its caller a LIVE result, not one freed under the
# caller's borrow. This is the splice manifestation of the native-tail-return
# defect that region-native-tail-return-uaf.lisp pins for the non-splice path:
# `(first ;argv)` lowers to `TailCallArrayMut` (build args array, splice-call),
# whose post-`TailCall` block — like the plain `TailCall` — needs a ReturnValue
# retain (`IncrefValueRegion`) on the native-completion fall-through, or the
# native's single pass-through reference is drained by the caller's
# `DecrefValueRegion`. The fix is in `lower_call`'s splice arm (src/lir/lower).
#
# WHY A CORRECTNESS GUARD, NOT A UAF WITNESS. Unlike the non-splice case, the
# splice path cannot fault TODAY even with the retain missing: the freshly-built
# splice args-array's region is never released (a separate, pre-existing leak,
# shared with the non-tail `CallArrayMut` path — see the discarded-tail-return
# leak the leak-suite canaries pin on this branch), and that live args-array keeps
# the borrowed result alive. So under `--trace=guardfree` the value reads back
# intact whether or not the retain is present — there is no deterministic fault
# to assert on. This test therefore asserts the RESULT VALUE is correct: GREEN
# now (the retain is present; and even without it the masking leak hid the bug),
# and it BECOMES a real use-after-free guard the moment the args-array leak is
# fixed — at which point a missing retain would free the result under the
# caller's borrow and this assert would read torn bytes. Keeping the retain in
# now means that future leak fix lands UAF-free. docs/impl/region-rules.md Rules 4/5/8.
#
# Run under the guardfree oracle in tests/integration/elle_scripts.rs, mirroring
# region-native-tail-return-uaf.

# ── controls: spliced accessor result CONSUMED by a borrowing native ───────────
# `length` borrows the heap result and returns an immediate; correct regardless
# of the retain (the result is not tail-returned).
#
# Loop sizing: hundreds of iterations is far past the adaptive-JIT threshold
# (10 calls) while keeping the whole file inside the guardfree mapping budget
# (vm.max_map_count): the oracle leaks one PROT_NONE mapping per FREED region
# page, so a reclaiming stdlib-heavy loop consumes mappings per iteration and
# an oversized count aborts on mmap exhaustion, not a UAF.

(defn ctl_first (argv)
  (length (first ;argv)))
(defn ctl_get (argv)
  (length (get ;argv)))

# ── subjects: TAIL-return the spliced heap pass-through result ──────────────────
(defn ret_first (argv)
  (first ;argv))
# (first coll)
(defn ret_get (argv)
  (get ;argv))
# (get coll 0)

# ── controls run first (must stay correct) ─────────────────────────────────────
(var c 0)
(var rc1 0)
(while (%lt c 500)
  (assign rc1 (ctl_first (list (list (concat "xy" "z")))))
  (assign rc1 (ctl_get (list [(concat "xy" "z")] 0)))
  (assign c (%add c 1)))
(assert (= rc1 3)
        "control: borrowing-consumer spliced accessor mis-read (harness broken)")

# ── subjects: each spliced tail-returned heap result must survive the borrow ────
(var i 0)
(var a "")
(var b "")
(while (%lt i 500)
  (assign a (ret_first (list (list (concat "xy" "z")))))
  (assign b (ret_get (list [(concat "xy" "z")] 0)))
  (assign i (%add i 1)))
(assert (= a "xyz")
        "(first ;argv) tail-returned: result freed under the caller's borrow")
(assert (= b "xyz")
        "(get ;argv) tail-returned: result freed under the caller's borrow")

(println "region-splice-tail-return: ok")
