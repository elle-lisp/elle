(elle/epoch 12)
# audited: 2026-09-23
# A wrapper call over two proven numbers dissolves to its %-op, and computes what
# the wrapper computes.
#
# Each wrapper is spelled three ways. `dissolved` tests both operands for numbers
# without raising, so the grafted wrapper's own guards fold away and only the op
# is left. `grafted` proves nothing, so the graft keeps the wrapper's guards. The
# wrapper itself, handed to `apply` as a value, runs the stdlib closure. The three
# are compared on every pair below. Which code the compiler emits is part of the
# subject here, so the first section reads the disassembly.
# docs/impl/graft.md

(defn disasm-text [f]
  "The disassembly of f's bytecode, as one string."
  (string/join (fn/disasm f) " "))

# A two-operand function that proves both operands numbers, and raises nothing.
(defmacro dissolved (op)
  `(fn [a b] (if (and (number? a) (number? b)) (,op a b) :not-numbers)))

# A two-operand function that proves nothing about its operands.
(defmacro grafted (op)
  `(fn [a b] (,op a b)))

(def wrappers
  [[:+ + (dissolved +) (grafted +)] [:- - (dissolved -) (grafted -)]
   [:* * (dissolved *) (grafted *)] [:< < (dissolved <) (grafted <)]
   [:> > (dissolved >) (grafted >)] [:<= <= (dissolved <=) (grafted <=)]
   [:>= >= (dissolved >=) (grafted >=)] [:= = (dissolved =) (grafted =)]])

# ── The proven spelling keeps no guard; the unproven one keeps them ──

# Every guard a grafted wrapper brings raises through an Emit. `=` has no guard
# to keep, so it is not in this section; its number test is pinned in
# hir::typeinfer::tests::graft.
#
# The counter-factual: while a wrapper call stayed a call, its guards lived in
# the wrapper's own body, so the unproven spelling held no Emit either.
(each [name _ proven unproven] (filter (fn [w] (not= (first w) :=)) wrappers)
  (assert (not (string/contains? (disasm-text proven) "Emit"))
          (string name " over two proven numbers keeps no guard"))
  (assert (string/contains? (disasm-text unproven) "Emit")
          (string name " over unproven operands keeps its guards")))

# ── All three spellings compute one value ─────────────────────────

(def nan (/ 0.0 0.0))
(def inf (math/inf))

# identical? compares floats by bit pattern, so it tells -0.0 from 0.0, an int
# from a float, and one NaN from another.
(def pairs
  [[1 2] [-7 4] [0 0] [9223372036854775807 1] [-9223372036854775808 -1] [1.5 2]
   [2 1.5] [1 1.0] [-0.0 -0.0] [-0.0 0.0] [0.0 -0.0] [0 -0.0] [-0.0 0] [nan 1.0]
   [1 nan] [nan nan] [inf (- inf)] [inf 1]])

(each [name wrapper proven unproven] wrappers
  (each [a b] pairs
    (let [low (proven a b)
          graft (unproven a b)
          call (apply wrapper [a b])]
      (assert (identical? low graft)
              (string "(" name " " a " " b "): dissolved " low ", grafted "
                      graft))
      (assert (identical? graft call)
              (string "(" name " " a " " b "): grafted " graft ", called " call)))))

# ── The sum keeps a negative zero ─────────────────────────────────

# The counter-factual: while the + wrapper summed from the integer 0, the called
# wrapper returned 0.0 here, and the op returned -0.0. Dissolving + would have
# changed the sign of a zero sum.
(assert (identical? (apply + [-0.0 -0.0]) -0.0)
        "the + wrapper keeps a negative zero sum")
(assert (identical? (+ -0.0) -0.0) "the + wrapper returns its one operand")
(assert (identical? (+) 0) "the empty sum is the integer 0")
(let [[ok? err] (protect ((fn [x] (+ x)) "s"))]
  (assert (not ok?) "one non-number operand still raises")
  (assert (= (get err :error) :type-error)
          (string "expected :type-error, got " err)))
