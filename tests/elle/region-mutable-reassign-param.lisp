(elle/epoch 12)
# A REASSIGNED MUTABLE PARAMETER (`[@x]`) whose post-reassign value is MOVED into a
# tail call must survive the move — the tail-move retain must order ahead of the
# param cell's last-use release. Guards the owned-params "value-freed-under-a-move"
# surface, the same family region-tail-move-borrow-uaf.lisp pins.
#
# Mechanism (docs/impl/region/rules.md Rules 4, 5, 8):
#   - A mutable param `@x` is materialized as a capture cell the callee OWNS
#     (`push_param` wraps the incoming arg; the cell's cross-region incref counts it).
#   - `(assign x …)` overwrites the cell (UpdateCapture: decref the displaced prior,
#     incref the new value), so the cell now holds the new value at rc 1.
#   - The body's `(… x)` tail call MOVES that new value to the callee. A tail move of
#     a borrowed cell value emits a retain (the callee's fresh owning reference) AND
#     the param cell reaches its last use, so the cell's `DecrefCellRegion` fires —
#     its cascade frees the cell's contents, the very value being moved. The retain
#     must order BEFORE that release; emitting the release first frees the value and
#     the retain reads a freed page. `lower_call` defers the tail arg's decrefs so the
#     retain precedes them.
#
# Under `--trace=guardfree` a regression faults (SIGSEGV, cascade free) at the stale
# read; on plain VM the freed page is stale-but-intact and the functional asserts
# below still pass, so guardfree is the robust oracle (as in
# region-tail-move-borrow-uaf.lisp). Pinned under the oracle by
# `region_mutable_reassign_param_uaf` (tests/integration/elle_scripts.rs).

# ── subjects ──────────────────────────────────────────────────────
# (a) overwrite an owned heap arg with a fresh heap value, return the new one.
(defn repl [@x]
  (assign x (@array 7 8 9))
  x)

# (b) multi-reassign chain: each overwrite must release exactly the displaced
# prior — never the still-incoming original twice.
(defn chain [@x]
  (assign x (@array 1))
  (assign x (@array 2 2))
  (assign x (@array 3 3 3))
  (length x))

# (c) clobber an ALIASED arg: the caller still holds the value after the callee
# reassigns its param. The owned-params incref must keep the caller's reference
# live; a double-release frees it under the caller.
(defn clobber [@x]
  (assign x (@array 0 0 0))
  (length x))

# ── witnesses ─────────────────────────────────────────────────────
(assert (= (length (repl (@array 1 2))) 3)
        "reassigned param: the returned reassigned value was freed")
(assert (= (chain (@array 9 9 9 9)) 3)
        "reassigned param chain: an overwrite released the wrong prior")

(let [@a (@array 5 5)]
  (assert (= (clobber a) 3) "clobber returned a freed reassigned value")  # The KEY UAF witness: `a` must survive the call that reassigned clobber's param.
  (assert (= (length a) 2)
          "an aliased arg was freed under the caller by the param's double-release"))

# (d) recycle physical ids in a loop so a double-free lands on a reused region.
(def before-reg (arena/region-count))
(var i 0)
(while (%lt i 2000)
  (repl (@array 1 2 3))
  (assign i (%add i 1)))
# Per-call values must all reach 0 — no growth (the double-free path also leaks
# distinct ids under guardfree, where frees are withheld).
(assert (%lt (%sub (arena/region-count) before-reg) 50)
        "reassigned-param loop leaked regions")

(println "region-mutable-reassign-param: ok")
