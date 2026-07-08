(elle/epoch 12)
# A local mutual-recursion clique (`ev`/`od`) whose `letrec` BODY ends in a tail
# call to a NON-member must reclaim its merged arena soundly — no premature free.
#
# The closure-cycle merge collapses the `ev`/`od` SCC and their forward cells onto
# one arena, freed once by the arena's binding-scope `DecrefRegion`. When the body
# is a frame-replacing tail call, that drop is dead code past the `TailCall`, so the
# release rides the explicit arena adopt (`TailCall::adopt_region_slot`,
# `RegionInfo::cycle_tail_adopt`): a CLOSURE callee's new activation adopts the arena
# and frees it at the recursion's completion; a NATIVE callee never replaces the
# frame and falls through to the live scope-exit drop. The two are mutually exclusive
# per call, so exactly one release fires however the callee resolves at runtime — the
# compiler never classifies the callee (docs/impl/region/letrec.md § The letrec
# closure-cycle merge). A premature free would leave `ev`/`od` (whose regions ARE the
# merged arena) dereferencing recycled pages on the next recursion step — a stale
# closure-env read: a generation panic on the plain VM, a SIGSEGV under
# `--trace=guardfree`. Each clique returns `n mod 2` (base cases 0/1), so a corrupt
# read also shows as a wrong value.
#
# Covers every non-member body-tail callee kind, a MIXED body (member + non-member
# arm — exactly one release per path), and the same clique rebuilt PER LOOP ITERATION
# (per-call reclamation at recursion-completion granularity, not activation
# granularity — the case an activation-owner-node cut would leak/double-free). Pinned
# under the UAF oracle by `region_native_tail_mutual_cycle_uaf`
# (tests/integration/elle_scripts.rs), run on both `--checked-intrinsics` settings.

# A foreign non-member closure (its `(g …)` tail replaces the frame, so the adopt is
# the release). `g` returns its argument.
(defn g [x]
  x)

# Native body tail: `(%add (ev n) 0)` is a `Call` to the `%add` native checked-on
# (an inlined `Intrinsic` checked-off) — the frame is not replaced, the live
# scope-exit drop frees the arena.
(defn f-native [n]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (%add (ev n) 0)))

# Redefined-operator body tail: the stdlib redefines `+`'s binding to a bytecode
# CLOSURE, so `(+ (ev n) 0)`'s tail IS frame-replacing and the adopt is the release.
# `is_primitive` is set on `+` yet the gate treats it as an ordinary non-member.
(defn f-op [n]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (+ (ev n) 0)))

# Foreign-closure body tail: `(g (ev n))` replaces the frame with `g`; the adopt
# frees the arena at `g`'s (the recursion's) completion.
(defn f-foreign [n]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (g (ev n))))

# Mixed body tail: one arm tail-calls a MEMBER (`ev`, released by the
# stranded-cycle adopt, its binding-scope drop dead there), the other a NON-member
# (`%add`, released by the live scope-exit drop). Exactly one release fires per path;
# both `ev` calls return `n mod 2`, so the result is path-independent.
(defn f-mixed [n take-member]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (if take-member (ev n) (%add (ev n) 0))))

# Drive one clique builder in a loop, rebuilding its arena every iteration and
# asserting each result is `n mod 2` (a premature free faults or corrupts here).
(defn drive [label mk]
  (def @i 0)
  (while (%lt i 60)
    (assert (= (mk i) (mod i 2))
            (string label ": wrong result at i=" i " (arena freed early?)"))
    (assign i (%add i 1))))

(drive "native" f-native)
(drive "op" f-op)
(drive "foreign" f-foreign)
# Mixed: alternate which arm the body takes each iteration; both give n mod 2.
(def @i 0)
(while (%lt i 60)
  (assert (= (f-mixed i (= (mod i 2) 0)) (mod i 2))
          (string "mixed: wrong result at i=" i " (arena freed early?)"))
  (assign i (%add i 1)))

# Accumulate across a longer loop so the arena is minted-and-freed hundreds of times
# (a leak would grow RSS, a double-free would fault) — the per-iteration granularity
# the recursion-completion release provides.
(def @sum 0)
(def @k 0)
(while (%lt k 300)
  (assign sum (%add sum (f-foreign k)))
  (assign k (%add k 1)))
(assert (= sum 150) "foreign loop: sum of (k mod 2) for k in 0..300 must be 150")

(println "region-native-tail-mutual-cycle-uaf: ok")
