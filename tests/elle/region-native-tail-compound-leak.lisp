(elle/epoch 12)
# CANARY + COUNTERFACTUAL (open defect, shrink-only — docs/impl/region/diagnostics.md
# § Validation). Pins the PRECISE disposition of the native-tail-return leak for
# the funnel-call `%`-ops (%put/%del — every storing/removing op lowers as the
# native funnel Call; docs/intrinsics.md § Lowering):
#
#                         bare tail body        nested in begin/if/cond
#   funnel-call %-op      BOUNDED               LEAK   (1 region/iter)
#   opcode %-op (%pair)   BOUNDED               BOUNDED
#
# Mechanism (LIR- and ANF-confirmed). A native tail call emits its result
# retain in the post-`TailCall` block (`IncrefValueRegion` = retain A,
# src/lir/lower/control/call.rs:130), which RUNS on the native-completion
# fall-through (a native pushes no bytecode frame). When the call is the
# lambda's direct body, that single retain is the whole return convention and
# the discarding caller's one `DecrefValueRegion` balances it — BOUNDED. When
# the call is nested in a `begin`/`if`/`cond`, ANF names the branch/sequence
# value in a temp `_t` (visible under `--dump=fhir`:
# `(let [_t (/*tail*/ %op …)] (return _t))`); the value reaching `return` is
# now a plain `Var`, which `wrap_tail_returns` DOES wrap → a SECOND
# `IncrefValueRegion` (retain B, src/hir/return_incref.rs). Retained TWICE
# (A + B), released ONCE: one region stranded per call. A bare or
# direct-`let`-body tail call stays in return position, where
# `wrap_tail_returns` skips it (`is_wrappable` excludes `Call{is_tail:true}`)
# — only A, BOUNDED. A CLOSURE tail call never leaks: its post-`TailCall`
# block is dead (frame replaced). Effect-independent: Fresh/Funnel/Mixed leak
# identically when nested. Measuring through a stdlib wrapper — whose body
# tail-calls its intrinsic, putting the call in tail position no matter the
# outer placement — silently changes which shape is under test.
#
# A LEAK, not a UAF — `arena/region-count` delta. Each CANARY pins the open
# leak at its deterministic 1-region/iter rate; a fix drops it to ~0 and flips
# that assertion red, forcing it down to the bounded `(%lt … 100)` form.
#
# NOTE: the naive ANF guard (don't name a `Call{is_tail:true}`) makes the
# compound deltas bounded, but it UNMASKS a pass-through UAF (tests/elle/
# flow-graph.lisp faults at regionstore.rs:386) — removing the redundant
# retain leaves a borrowed pass-through result with too few claims (the §4.3
# hazard). The correct fix ADDS a consume of the callee's own claim while
# KEEPING the return mint, so pass-through results stay live.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

(defn near? [d target tol]
  (and (%ge d (%sub target tol)) (%le d (%add target tol))))

(def s {:a 1})

# subjects ─────────────────────────────────────────────────────────────────────
(defn bare-pair [a b]
  (%pair a b))
# Fresh,  bare
(defn bare-put [c k v]
  (%put c k v))
# Funnel, bare
(defn bare-del [c k]
  (%del c k))
# Mixed,  bare
(defn let-pair [a b]
  (let [x 5]
    (%pair a b)))
(defn mk [a b]
  (%pair a b))
(defn clo-if [a b]
  (if (%gt a 0) (mk a b) 0))
# closure tail in if
(defn begin-pair [a b]
  (begin
    0
    (%pair a b)))
(defn if-pair [a b]
  (if (%gt a 0) (%pair a b) 0))
(defn if-put [c k v]
  (if true (%put c k v) 0))
(defn if-del [c k]
  (if true (%del c k) 0))

(def bare-pair-d (measure (fn () (bare-pair 1 2)) 200 window))
(def bare-put-d (measure (fn () (bare-put s :b 2)) 200 window))
(def bare-del-d (measure (fn () (bare-del s :a)) 200 window))
(def let-pair-d (measure (fn () (let-pair 1 2)) 200 window))
(def clo-if-d (measure (fn () (clo-if 1 2)) 200 window))
(def begin-pair-d (measure (fn () (begin-pair 1 2)) 200 window))
(def if-pair-d (measure (fn () (if-pair 1 2)) 200 window))
(def if-put-d (measure (fn () (if-put s :b 2)) 200 window))
(def if-del-d (measure (fn () (if-del s :a)) 200 window))

(println "region-native-tail-compound-leak deltas over " window " iters:")
(println "  bare %pair  : " bare-pair-d "   bare %put : " bare-put-d
         "   bare %del : " bare-del-d)
(println "  let  %pair  : " let-pair-d "   clo-in-if : " clo-if-d)
(println "  begin %pair : " begin-pair-d "   if %pair : " if-pair-d
         "   if %put : " if-put-d "   if %del : " if-del-d)

# %pair (opcode, Fresh) is bounded bare and in compounds, and a closure tail
# call and a let-body never leak — universal guards.
(assert (%lt bare-pair-d 100)
        (concat "bare %pair leaks, delta=" (number->string bare-pair-d)))
(assert (%lt let-pair-d 100)
        (concat "let-body %pair leaks, delta=" (number->string let-pair-d)))
(assert (%lt clo-if-d 100)
        (concat "closure tail in if leaks, delta=" (number->string clo-if-d)))
(assert (%lt begin-pair-d 100)
        (concat "begin-nested %pair leaks, delta=" (number->string begin-pair-d)))
(assert (%lt if-pair-d 100)
        (concat "if-nested %pair leaks, delta=" (number->string if-pair-d)))

# Funnel-call ops: bare BOUNDED, compound LEAKS (the double-mint canary).
(assert (%lt bare-put-d 100)
        (concat "bare %put should be bounded, delta="
                (number->string bare-put-d)))
(assert (%lt bare-del-d 100)
        (concat "bare %del should be bounded, delta="
                (number->string bare-del-d)))
(assert (near? if-put-d window 100)
        (concat "CANARY (native tail in if, %put Funnel): expected ~"
                (number->string window) ", got " (number->string if-put-d)
                " — if fixed, change to `(%lt … 100)`"))
(assert (near? if-del-d window 100)
        (concat "CANARY (native tail in if, %del Mixed): expected ~"
                (number->string window) ", got " (number->string if-del-d)))

(println "region-native-tail-compound-leak: ok")
