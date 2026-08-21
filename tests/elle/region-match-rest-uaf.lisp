(elle/epoch 12)
# Counterfactual: a `match` pattern binding that ALIASES into the scrutinee's
# region — `(a & rest)` binds `rest` to a sublist sharing the subject list's
# cells; `(h . t)`, an immutable-array element `[a b]`, an immutable-struct value
# `{:k v}` likewise hand back a pointer co-located in the subject's region pages
# (docs/impl/region/model.md § "RegionSlice contents share their object's region"). The
# region solver's `Match` arm (src/hir/regions.rs) DISCARDED the scrutinee's
# regions (`let _ = self.walk(value)`) and recorded every pattern binding with an
# EMPTY `binding_regions`, so the subject region's `decref_point` was NOT extended
# over uses of the bound alias. The subject region is then freed at the match's
# own `decref_point` while the bound sublist/element is still live — use-after-free.
#
# This is the `match` sibling of the `Destructure` HIR node (`(def (a & r) …)`),
# which DOES propagate the source's regions to its pattern bindings (regions.rs,
# the `HirKind::Destructure` arm: "Destructured bindings may hold values that live
# in the source's region(s) … Conservatively propagate the source's regions").
# The fix makes the `Match` arm mirror `Destructure` exactly: union the
# scrutinee's regions into each arm binding's `binding_regions`, so the subject
# region's `decref_point` extends through every use of the bound alias and the
# normal cross-region increfs fire on escape.
#
# Mechanism (witnessed under `--trace=guardfree`, bisected to the match binding):
#   free site: `DecrefValueRegion of list (runtime region N) @ <the match>`,
#   the consumer then reads the freed sublist/element.
#
# `length` is the consumer: it borrows the bound alias and returns an IMMEDIATE,
# so the only region that can be freed under the borrow is the subject's — exactly
# isolating the missing decref_point extension (mirrors region-array-element-uaf).
# A fresh heap subject is built each iteration so an over-early free faults on the
# next read. RED (SIGSEGV under guardfree) before the fix; GREEN once the solver
# extends the subject region's lifetime over the bound alias.

# ── witnesses: aliasing pattern bindings consumed AFTER the match ──────────────
# Each binds an alias into the scrutinee region, then `length`s it post-match.

# (a) list `& rest` — RestDestructure hands back the shared tail cons.
(defn w_list_rest ()
  (length (match (list 1 2 3)
            (a & rest) rest
            _ :fail)))

# (b) pair tail `(h . t)` — RestDestructure on a `pair`.
(defn w_pair_tail ()
  (length (match (pair 1 (list 2 3))
            (h . t) t
            _ :fail)))

# (c) list head `(h & t)` binding a heap ELEMENT — FirstDestructure hands back an
# element co-located in the list's region.
(defn w_list_head ()
  (length (match (list (list 7 7) 2 3)
            (h & t) h
            _ :fail)))

# (d) immutable-array element `[a b]` — ArrayMutRefDestructure on an array whose
# element is a heap value laid out in the array's own region.
(defn w_arr_elem ()
  (length (match [(list 7 7) 2]
            [a b] a
            _ :fail)))

# (e) immutable-struct value `{:k v}` — StructGetOrNil hands back a co-located value.
(defn w_struct_val ()
  (length (match {:k (list 4 5)}
            {:k v} v
            _ :fail)))

# (f) the original advanced.lisp shape: a GUARD arm that returns `& rest`.
(defn w_guard_rest ()
  (length (match (list 1 2 3)
            (a & rest) when
            (> a 0) rest
            _ :fail)))

# ── controls: the SAME extraction via native `rest`/`first`/`get`, which DO the
# pass-through retain (docs/impl/region/rules.md Rule 5). Correct NOW — bisection that the
# defect is the match binding, not aliasing access in general.
(defn c_rest ()
  (length (rest (list 1 2 3))))
(defn c_first ()
  (length (first (list (list 7 7) 2 3))))
(defn c_get ()
  (length (get [(list 7 7) 2] 0)))

# ── drive: fresh subject each iteration; an over-early free faults on next read ──
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(var h 0)
(var k 0)
(while (%lt i 3000)
  (assign a (w_list_rest))
  (assign b (w_pair_tail))
  (assign c (w_list_head))
  (assign d (w_arr_elem))
  (assign e (w_struct_val))
  (assign f (w_guard_rest))
  (assign g (c_rest))
  (assign h (c_first))
  (assign k (c_get))
  (assign i (%add i 1)))

# Controls: native pass-through retains, correct now (harness sanity).
(assert (= g 2) "control: (rest list) tail mis-read (harness broken)")
(assert (= h 2) "control: (first list) heap element mis-read (harness broken)")
(assert (= k 2) "control: (get arr 0) element mis-read (harness broken)")

# Witnesses: a match-bound alias must survive its consuming use.
(assert (= a 2)
        "list `& rest` alias over-released — subject region freed under length's borrow")
(assert (= b 2)
        "pair `. t` alias over-released — subject region freed under the borrow")
(assert (= c 2)
        "list head heap-element alias over-released — subject region freed under the borrow")
(assert (= d 2)
        "imm-array element alias over-released — subject region freed under the borrow")
(assert (= e 2)
        "imm-struct value alias over-released — subject region freed under the borrow")
(assert (= f 2)
        "guard-arm `& rest` alias over-released — the advanced.lisp UAF")

(println "region-match-rest-uaf: ok")
