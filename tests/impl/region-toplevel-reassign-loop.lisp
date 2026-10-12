(elle/epoch 14)
# audited: 2026-10-11
# A module-scope mutable reassigned in a loop releases each displaced value at
# its overwrite, whatever produced the value.
#
# docs/impl/region/bindings.md
#
# A fresh local allocation is donated to the cell, and a call result takes the
# counted store with its producer release pinned to the store site. The
# counter-factual: that release suppressed beside the counted store leaves one
# region per trip, so a 220-trip loop reads about 200 where it must read about
# 0, and 100,000 trips of (assign s [i]) held 495 MB resident.
#
# Every subject is a TOP-LEVEL binding, because a fn-local cell is the other
# half of the model and was bounded before this file. Each subject warms up for
# 20 trips, reads the live-region count, runs 200 more trips, and reads it
# again. The reads of each stored value run under guardfree, so a release that
# fired too early faults instead of reading back a recycled page.

# ── the gauge is live: a chained accumulator keeps every prior ───────────
(def @chain nil)
(def @n 0)
(while (%lt n 20)
  (assign chain (%pair n chain))
  (assign n (%add n 1)))
(def chain-before (arena/region-count))
(while (%lt n 220)
  (assign chain (%pair n chain))
  (assign n (%add n 1)))
(def chain-growth (%sub (arena/region-count) chain-before))
(assert (%gt chain-growth 150)
        (string "the gauge must see a live chain grow by about 200 regions, got "
                chain-growth))

# ── 1. the control: a %pair opcode's region is donated ───────────────────
(def @p nil)
(def @i1 0)
(while (%lt i1 20)
  (assign p (%pair i1 i1))
  (assign i1 (%add i1 1)))
(def p-before (arena/region-count))
(while (%lt i1 220)
  (assign p (%pair i1 i1))
  (assign i1 (%add i1 1)))
(def p-growth (%sub (arena/region-count) p-before))
(assert (= (first p) 219) "the donated cell holds its last value")
(assert (%lt p-growth 20)
        (string "a donated %pair strands its priors: live regions grew by "
                p-growth))

# ── 2. an array literal is a native call's result ────────────────────────
(def @a nil)
(def @i2 0)
(while (%lt i2 20)
  (assign a [i2])
  (assign i2 (%add i2 1)))
(def a-before (arena/region-count))
(while (%lt i2 220)
  (assign a [i2])
  (assign i2 (%add i2 1)))
(def a-growth (%sub (arena/region-count) a-before))
(assert (= (get a 0) 219) "the counted cell holds its last array")
(assert (%lt a-growth 20)
        (string "an array literal strands its priors: live regions grew by "
                a-growth))

# ── 3. the list wrapper ──────────────────────────────────────────────────
(def @l nil)
(def @i3 0)
(while (%lt i3 20)
  (assign l (list i3 i3))
  (assign i3 (%add i3 1)))
(def l-before (arena/region-count))
(while (%lt i3 220)
  (assign l (list i3 i3))
  (assign i3 (%add i3 1)))
(def l-growth (%sub (arena/region-count) l-before))
(assert (= (first l) 219) "the counted cell holds its last list")
(assert (%lt l-growth 20)
        (string "a list strands its priors: live regions grew by " l-growth))

# ── 4. a string, through var ─────────────────────────────────────────────
(var s nil)
(var i4 0)
(while (%lt i4 20)
  (assign s (string "x" i4))
  (assign i4 (%add i4 1)))
(def s-before (arena/region-count))
(while (%lt i4 220)
  (assign s (string "x" i4))
  (assign i4 (%add i4 1)))
(def s-growth (%sub (arena/region-count) s-before))
(assert (= s "x219") "the counted cell holds its last string")
(assert (%lt s-growth 20)
        (string "a string strands its priors: live regions grew by " s-growth))

# ── 5. a struct literal ──────────────────────────────────────────────────
(def @m nil)
(def @i5 0)
(while (%lt i5 20)
  (assign m {:i i5})
  (assign i5 (%add i5 1)))
(def m-before (arena/region-count))
(while (%lt i5 220)
  (assign m {:i i5})
  (assign i5 (%add i5 1)))
(def m-growth (%sub (arena/region-count) m-before))
(assert (= (get m :i) 219) "the counted cell holds its last struct")
(assert (%lt m-growth 20)
        (string "a struct literal strands its priors: live regions grew by "
                m-growth))

# ── 6. a user function's result ──────────────────────────────────────────
(defn build [i]
  (list i i))
(def @u nil)
(def @i6 0)
(while (%lt i6 20)
  (assign u (build i6))
  (assign i6 (%add i6 1)))
(def u-before (arena/region-count))
(while (%lt i6 220)
  (assign u (build i6))
  (assign i6 (%add i6 1)))
(def u-growth (%sub (arena/region-count) u-before))
(assert (= (first u) 219) "the counted cell holds its last call result")
(assert (%lt u-growth 20)
        (string "a call result strands its priors: live regions grew by "
                u-growth))

# ── 7. an element a counted read aliased out of a call result ────────────
(defn pair-of [i]
  [[i] [i]])
(def @e nil)
(def @i7 0)
(while (%lt i7 20)
  (assign e (get (pair-of i7) 0))
  (assign i7 (%add i7 1)))
(def e-before (arena/region-count))
(while (%lt i7 220)
  (assign e (get (pair-of i7) 0))
  (assign i7 (%add i7 1)))
(def e-growth (%sub (arena/region-count) e-before))
(assert (= (get e 0) 219) "the counted cell holds its last element")
(assert (%lt e-growth 20)
        (string "an aliased element strands its priors: live regions grew by "
                e-growth))

# ── 8. a store on half the trips ─────────────────────────────────────────
(def @c nil)
(def @i8 0)
(while (%lt i8 20)
  (when (%eq (%rem i8 2) 0) (assign c [i8]))
  (assign i8 (%add i8 1)))
(def c-before (arena/region-count))
(while (%lt i8 220)
  (when (%eq (%rem i8 2) 0) (assign c [i8]))
  (assign i8 (%add i8 1)))
(def c-growth (%sub (arena/region-count) c-before))
(assert (= (get c 0) 218) "the conditionally stored cell holds its last array")
(assert (%lt c-growth 20)
        (string "a conditional store strands its priors: live regions grew by "
                c-growth))

# ── 9. two cells interleaved read their own values ───────────────────────
(def @x nil)
(def @y nil)
(def @i9 0)
(while (%lt i9 20)
  (assign x [i9])
  (assign y (list i9))
  (assign i9 (%add i9 1)))
(def xy-before (arena/region-count))
(while (%lt i9 220)
  (assign x [i9])
  (assign y (list i9))
  (assign i9 (%add i9 1)))
(def xy-growth (%sub (arena/region-count) xy-before))
(assert (= (get x 0) 219)
        "the first of two interleaved cells holds its own value")
(assert (= (first y) 219)
        "the second of two interleaved cells holds its own value")
(assert (%lt xy-growth 20)
        (string "two interleaved cells strand their priors: live regions grew by "
                xy-growth))

(println "region-toplevel-reassign-loop: ok")
