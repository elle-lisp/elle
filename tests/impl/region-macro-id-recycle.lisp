(elle/epoch 12)
# audited: 2026-09-14
# What a macro expansion costs in physical region IDS — the fourth heap
# dimension (docs/impl/region/model.md § "Physical id recycling").
#
# An expansion wraps each of its arguments as a `Value` born in a transient
# region the scope mints. An atom argument becomes an immediate rather than a
# heap value, so a macro call whose arguments are all atoms wraps nothing and
# leaves that region unmaterialized: no entry, no page, no object, no reference
# count. `arena/count`, `arena/region-count` and `arena/bytes` are therefore all
# blind to it, and they stay flat while every expansion raises the largest id a
# later mint hands out — one `Option<RegionEntry>` table slot of resident memory
# each, for as long as the process keeps compiling.
#
# `arena/region-ids` is the gauge for that dimension. It reads `next_physical`,
# which a mint raises only when the free list is empty, so a steady-state loop
# holds it flat and every unit of growth is an id that did not come back. It is
# Immediate, so sampling it allocates nothing and does not perturb the
# measurement.
#
# Every bound below is a CEILING, so the file is shrink-only. The store-level
# contract — that the recycle reissues an unmaterialized id and refuses every
# id already booked — is pinned in Rust by `regionstore::tests::recycle`, and
# what the scope's open and close owe each other by `arena::tests::macroscope`.

# The window is sized against a DEBUG binary's clock rather than against what
# sharpens the reading: the per-file corpus pass runs this file unoptimized under
# a 30-second timeout, and every subject here drives an `eval`, which costs some
# 2 ms there. What the window has to buy is separation from the `settle` ceiling
# below, and it buys that at any size worth running.
(def window 300)
(def warm 50)

(defn ids [thunk]
  "Physical ids issued over WINDOW calls of THUNK, after WARM untimed calls."
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-ids))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-ids) before))

# the gauge-live discriminator ─────────────────────────────────────────────────
# A ceiling reads green against a DEAD gauge too, so measure a shape whose id
# cost cannot be zero first and require that it moves. A module-level sink keeps
# every region it is handed, so nothing is freed, the free list drains, and every
# later mint has to take a fresh id. If this reads under one id per call the gauge
# is not measuring and every ceiling below is void.

(def @id-sink @[])

(def live-d (ids (fn [] (push id-sink (pair 1 2)))))
(println "region-macro-id-recycle: discriminator (retained cons) issued " live-d
         " ids over " window " calls")
(assert (%ge live-d window)
        (concat "arena/region-ids is dead: a retained region per call issued only "
                (number->string live-d) " ids over " (number->string window)
                " calls — every ceiling below is void"))

# measurements ─────────────────────────────────────────────────────────────────
# `when`, `unless` and `case` called on atoms alone: the expansion wraps no
# argument, so its transient region is never materialized and its id must come
# back by the scope's own close rather than by a teardown that can never run.
#
# The counter-factual is the third group. `(when true (+ 1 2))` is the same macro
# one argument away: a compound argument IS wrapped, so the region materializes
# and the scope's reclaim frees it, which returns the id through the ordinary
# teardown. A run that reads zero for it and nonzero for the atom calls has
# measured exactly the unmaterialized exit and nothing else.

(def when-d (ids (fn [] (eval '(when true 1)))))
(def unless-d (ids (fn [] (eval '(unless false 1)))))
(def case-d
  (ids (fn []
         (eval '(case 1
                  1 2)))))
(def wrapped-d (ids (fn [] (eval '(when true (+ 1 2))))))
(def special-d (ids (fn [] (eval '(if true 1 2)))))

(println "  atom arguments:     (when true 1) " when-d "  (unless false 1) "
         unless-d "  (case 1 1 2) " case-d)
(println "  controls:           (when true (+ 1 2)) " wrapped-d
         "  (if true 1 2) " special-d)

# The ceiling is a constant rather than zero, and the constant is what makes the
# reading a per-call one. A loop's first turns hold one or two more regions at
# once than the free list was carrying, and each of those raises `next_physical`
# once for the whole run — a settling cost that does not grow with the window. A
# shape that strands an id per CALL issues one per call, so it reads the whole
# window instead, and the gap between the two widens with every call added.
(def settle 8)

(defn at-most [d label]
  (assert (%le d settle)
          (concat label " issued " (number->string d) " ids over "
                  (number->string window) " calls, over the ceiling of "
                  (number->string settle)
                  " — an id per call is an id that never came back")))

(at-most when-d "(when true 1), whose arguments are both atoms")
(at-most unless-d "(unless false 1), whose arguments are both atoms")
(at-most case-d "(case 1 1 2), whose arguments are all atoms")
(at-most wrapped-d "(when true (+ 1 2)), whose body argument is wrapped")
(at-most special-d "(if true 1 2), which expands no macro at all")

# the dimension the other gauges cannot see ────────────────────────────────────
# The claim the file opens with, measured rather than asserted: the same loop
# that strands an id per call holds the object and region counts flat. Read this
# beside the discriminator above — a sink that grows both of them by one per call
# is what says these two gauges are alive.

# One loop, read on both gauges, rather than a helper called twice: each call
# would drive its own window of expansions, and the file's whole cost is the
# expansions it drives.
(var blind-warm 0)
(while (%lt blind-warm warm)
  (eval '(when true 1))
  (assign blind-warm (%add blind-warm 1)))
(def objs-before (arena/count))
(def regions-before (arena/region-count))
(var blind-i 0)
(while (%lt blind-i window)
  (eval '(when true 1))
  (assign blind-i (%add blind-i 1)))
(def blind-objs (%sub (arena/count) objs-before))
(def blind-regions (%sub (arena/region-count) regions-before))
(println "  the blind gauges:   objects " blind-objs "  regions " blind-regions
         " over " window " calls")
(assert (%le blind-objs settle)
        (concat "(when true 1) grew the object count by "
                (number->string blind-objs) " over " (number->string window)
                " calls — this file measures the id dimension, and a shape that "
                "moves the object count is a different defect"))
(assert (%le blind-regions settle)
        (concat "(when true 1) grew the region count by "
                (number->string blind-regions) " over " (number->string window)
                " calls — this file measures the id dimension, and a shape that "
                "moves the region count is a different defect"))

(println "region-macro-id-recycle: ok")
