(elle/epoch 12)
# audited: 2026-09-29
# A function whose tail is a spliced call to a heap pass-through native hands its caller a live result.
# docs/impl/region/mechanism.md
#
# The sidecar arms guardfree.
#
# `(first ;argv)` lowers to `TailCallArrayMut` (build the args array, then
# splice-call). Like the plain `TailCall`, its post-`TailCall` block takes a
# ReturnValue retain (`IncrefValueRegion`) on the native-completion
# fall-through, in `lower_call`'s splice arm (src/lir/lower).
# region-native-tail-return-uaf.lisp pins the same retain on the non-splice
# path.
#
# THE TRAP the assertions guard. The args array is reclaimed by the call that
# consumes it (docs/impl/region/mechanism.md), so nothing outlives the call to keep a
# borrowed result alive on the caller's behalf. Withhold the retain and the
# native's single pass-through reference is drained by the caller's
# `DecrefValueRegion`, and the reads below get torn bytes.
#
# THE COUNTER-FACTUAL. An args array that stranded instead of being reclaimed
# would hold the borrowed result, and these assertions would pass with the retain
# missing — a leak masking the defect. So this file is a witness only in company
# with region-splice-args.lisp, which pins the reclaim. docs/impl/region/rules.md
# Rules 4/5/8.

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
