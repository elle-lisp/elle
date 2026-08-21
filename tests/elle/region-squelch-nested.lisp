(elle/epoch 12)
# ── Region: a squelch closure shares its source's env backing region ──
#
# `(squelch f :yield)` builds a NEW closure that SHARES f's captured
# environment: `Closure { template: f.template.clone(), env: f.env, … }`
# (src/primitives/meta.rs `prim_squelch`). The env is an RegionSlice — a
# (ptr, len) pair — so the shared closure's env *backing data* stays in f's
# region, not its own. That is a Rule-5 cross-region escape: the squelch
# closure references f's region.
#
# The alloc-time cross-region scan (`find_object_cross_refs`, Closure arm)
# must incref that backing region. Without it, f's region is freed at its
# owning-scope `decref-region` (rc 1 → 0) right after the `def`, while the
# squelch closure still reads the shared env. The original symptom was the
# `protect`+`squelch`+nested-`yield` HANG: on the first fiber resume,
# `populate_env` reads f's freed env page → a mis-resolved Call target →
# infinite self-recursion (TCO loop = hang; non-tail = Rust stack overflow).
#
# Counter-factual: pre-fix every block below HANGS (the resume loop) or, in
# the non-tail block, overflows the Rust stack. The Rust-level pin is
# `closure_sharing_env_increfs_the_env_backing_region` in
# src/value/fiberheap/tests.rs. (NB: keep this allocation-light — a heavy
# protect-in-loop here trips a *separate*, still-open terminal-signal
# retention UAF specific to squelch-built errors; region-squelch-fiber-uaf.lisp.)

# The yielder is nested one call deep inside the squelched function, so the
# yield must propagate through `outer` before squelch converts it. squelch
# wraps the CALLER (`outer`), and the whole thing runs inside `protect`'s
# child fiber — the exact three-ingredient trigger (signals.lisp's hang).
(def inner (fn () (yield 1)))
(def outer (fn () (inner)))
(def safe (squelch outer :yield))

# `protect` runs `(safe)` in a child fiber; squelch converts the nested
# yield to a signal-violation, which protect catches as a failure.
(let [[ok? err] (protect (safe))]
  (assert (not ok?) "protect catches the squelched nested yield as failure")
  (assert (= (get err :error) :signal-violation)
          "squelch converts the nested yield to a signal-violation"))

# Non-tail form (`(let [r (inner)] r)`): pre-fix this overflowed the Rust
# stack instead of looping. Same defect, non-TCO shape.
(def outer2
  (fn ()
    (let [r (inner)]
      r)))
(def safe2 (squelch outer2 :yield))
(let [[ok? err] (protect (safe2))]
  (assert (not ok?) "non-tail squelch: failure")
  (assert (= (get err :error) :signal-violation)
          "non-tail squelch: signal-violation"))

# A squelch closure built from an INLINE lambda (its env captures the global
# `inner`) must also keep its shared env backing alive across the resume.
(let [s (squelch (fn () (inner)) :yield)
      [ok? err] (protect (s))]
  (assert (not ok?) "inline-lambda squelch: failure")
  (assert (= (get err :error) :signal-violation)
          "inline-lambda squelch closure reads its shared env intact"))

(println "region-squelch-nested: ok")
