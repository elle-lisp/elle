(elle/epoch 19)
# audited: 2026-11-24
# What a macro expansion costs in physical region IDS — the fourth heap
# dimension (docs/impl/region/model.md § "Physical id recycling").
#
# An expansion wraps each of its arguments as a syntax object born in a
# transient region the scope mints. A macro call with no arguments wraps
# nothing and leaves that region unmaterialized: no entry, no page, no object,
# no reference count. `arena/count`, `arena/region-count` and `arena/bytes` are
# therefore all blind to it, and they stay flat while every expansion raises the
# largest id a later mint hands out — one `Option<RegionEntry>` table slot of
# resident memory each, for as long as the process keeps compiling.
#
# `arena/region-ids` is the gauge for that dimension. It reads `next_physical`,
# which a mint raises only when the free list is empty, so a steady-state loop
# holds it flat and every unit of growth is an id that did not come back. The
# ratchet reads it as a rate per call against the rows of
# tests/ledger/region-macro-id-recycle.lisp. The store-level contract — that the
# recycle reissues an unmaterialized id and refuses every id already booked — is
# pinned in Rust by `regionstore::tests::recycle`, and what the scope's open and
# close owe each other by `arena::tests::macroscope`.
#
# A loop's first turns hold one or two more regions at once than the free list
# was carrying, and each of those raises `next_physical` once for the whole run.
# The first block of every rate is discarded, and that settling cost lands
# there; a shape that strands an id per CALL reads one per call in every block.
#
# The blocks are small because every probe drives an `eval`, which costs some
# 2 ms on a debug binary.

(def r ((import "std/ratchet")))

(defn id-rate [subject probe]
  (r:rate subject probe :on [r:ids] :block 50 :min 4 :max 8))

# A macro of no parameters, defined in the expander every `eval` shares.
(eval '(defmacro nothing []
         1))

# `(nothing)` wraps no argument, so its transient region is never materialized
# and its id must come back by the scope's own close rather than by a teardown
# that can never run.
#
# The same drive is read on the object and region counts too: the claim the
# file opens with, measured rather than asserted, is that the loop that strands
# an id per call holds those two flat.
(r:rate "(nothing)" (fn [j] (eval '(nothing))) :on [r:ids r:objects r:regions]
        :block 50 :min 4 :max 8)

# The counter-factual is the controls. `when`, `unless` and `case` called on
# atoms, and `(when true (+ 1 2))`, wrap their arguments, so the region
# materializes and the scope's reclaim frees it, which returns the id through
# the ordinary teardown. `(if true 1 2)` expands no macro at all. A run that
# reads zero for them and nonzero for `(nothing)` has measured exactly the
# unmaterialized exit and nothing else.
(id-rate "(when true 1)" (fn [j] (eval '(when true 1))))
(id-rate "(unless false 1)" (fn [j] (eval '(unless false 1))))
(id-rate "(case 1 1 2)"
         (fn [j]
           (eval '(case 1
                    1 2))))
(id-rate "(when true (+ 1 2))" (fn [j] (eval '(when true (+ 1 2)))))
(id-rate "(if true 1 2)" (fn [j] (eval '(if true 1 2))))

(println "region-macro-id-recycle: ok")
