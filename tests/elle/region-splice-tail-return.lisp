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
# THE TRAP the assertions guard. The args array is reclaimed by the call that
# consumes it (docs/impl/region/mechanism.md § "A spliced call's arguments come
# out of an array the convention owns"), so nothing outlives the call to keep a
# borrowed result alive on the caller's behalf. Withhold the retain and the
# native's single pass-through reference is drained by the caller's
# `DecrefValueRegion`, and the reads below get torn bytes.
#
# THE COUNTER-FACTUAL. While the array's own region was still stranded, that live
# array held the borrowed result and these same assertions passed with the retain
# missing — the leak masked the defect. So this file is a witness only in company
# with region-splice-args.lisp: a regression that re-strands the array would paint
# it green again. docs/impl/region/rules.md Rules 4/5/8.
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
