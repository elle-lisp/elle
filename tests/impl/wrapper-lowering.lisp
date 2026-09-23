(elle/epoch 12)
# audited: 2026-09-23
# A wrapper call over two proven numbers lowers to its %-op, and computes what
# the wrapper call computes.
#
# Each wrapper is built twice. `guarded` proves both operands with a diverging
# guard, so its tail call to the wrapper lowers to the op. `unproven` proves
# nothing, so its call stays a call. The two are compared on every pair below.
# Which code the compiler emits is part of the subject here, so the first
# section reads the disassembly.
# docs/intrinsics.md

(defn disasm-text [f]
  "The disassembly of f's bytecode, as one string."
  (string/join (fn/disasm f) " "))

# A two-operand function whose guards prove both operands numbers.
(defmacro guarded (op)
  `(fn [a b]
     (when (%not (number? a)) (error {:error :type-error :message "a"}))
     (when (%not (number? b)) (error {:error :type-error :message "b"}))
     (,op a b)))

# A two-operand function that proves nothing about its operands.
(defmacro unproven (op)
  `(fn [a b] (,op a b)))

(def wrappers
  [[:+ (guarded +) (unproven +)] [:- (guarded -) (unproven -)]
   [:* (guarded *) (unproven *)] [:< (guarded <) (unproven <)]
   [:> (guarded >) (unproven >)] [:<= (guarded <=) (unproven <=)]
   [:>= (guarded >=) (unproven >=)] [:= (guarded =) (unproven =)]])

# ── The guarded spelling lowers; the unproven one keeps the call ──

# The guards call number?, so both spellings hold a Call. Only a wrapper call
# left in tail position is a TailCall, and a lowered op ends in Return.
(each [name lowered wrapped] wrappers
  (assert (not (string/contains? (disasm-text lowered) "TailCall"))
          (string name " over two guarded numbers lowers to its op"))
  (assert (string/contains? (disasm-text wrapped) "TailCall")
          (string name " over unproven operands stays a wrapper call")))

# ── Both spellings compute one value ──────────────────────────────

(def nan (/ 0.0 0.0))
(def inf (math/inf))

# identical? compares floats by bit pattern, so it tells -0.0 from 0.0, an int
# from a float, and one NaN from another.
(def pairs
  [[1 2] [-7 4] [0 0] [9223372036854775807 1] [-9223372036854775808 -1] [1.5 2]
   [2 1.5] [1 1.0] [-0.0 -0.0] [-0.0 0.0] [0.0 -0.0] [0 -0.0] [-0.0 0] [nan 1.0]
   [1 nan] [nan nan] [inf (- inf)] [inf 1]])

(each [name lowered wrapped] wrappers
  (each [a b] pairs
    (let [low (lowered a b)
          call (apply wrapped [a b])]
      (assert (identical? low call)
              (string "(" name " " a " " b "): lowered " low ", wrapper " call)))))

# ── The sum keeps a negative zero ─────────────────────────────────

# The counter-factual: while the + wrapper summed from the integer 0, the
# wrapper call returned 0.0 here, and the op returned -0.0. Lowering + would
# have changed the sign of a zero sum.
(def unproven-add (get (get wrappers 0) 2))
(assert (identical? (apply unproven-add [-0.0 -0.0]) -0.0)
        "the + wrapper keeps a negative zero sum")
(assert (identical? (+ -0.0) -0.0) "the + wrapper returns its one operand")
(assert (identical? (+) 0) "the empty sum is the integer 0")
(let [[ok? err] (protect ((fn [x] (+ x)) "s"))]
  (assert (not ok?) "one non-number operand still raises")
  (assert (= (get err :error) :type-error)
          (string "expected :type-error, got " err)))
