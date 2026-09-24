(elle/epoch 12)
# audited: 2026-09-23
# A grafted call computes and raises what the call it replaced computed and raised.
#
# Every call below to a stdlib wrapper, or to a function this file defines, is
# replaced by the callee's body, and the proven types then delete the branches
# the operands cannot take. None of that may change a value or an error. The
# traps are the ones the fold can hit: a guard certain to raise must still
# raise, a branch no value reaches must go without taking a binding the rest of
# the program reads, and a guard must still narrow through the wrappers it is
# written with.
# docs/impl/graft.md

(defn raises [thunk]
  "The :error kind thunk raises, or nil when it returns."
  (let [[ok? err] (protect (thunk))]
    (if ok? nil (get err :error))))

# ── A guard certain to raise still raises ─────────────────────────

(assert (= (raises (fn [] (+ "a" 1))) :type-error) "+ over a string raises")
(assert (= (raises (fn [] (- :k))) :type-error) "negating a keyword raises")
(assert (= (raises (fn [] (< "a" 1))) :type-error)
        "a string and an int do not compare")
(assert (= (raises (fn [] (put {} :a 1 2))) :arity-error)
        "put takes at most three")

# ── Every arity computes what the wrapper computes ────────────────

(assert (= (+) 0) "the empty sum")
(assert (= (*) 1) "the empty product")
(assert (= (- 5) -5) "one operand negates")
(assert (= (+ 1 2 3) 6) "the general arm sums the rest")
(assert (= (- 10 1 2) 7) "the general arm subtracts left to right")
(assert (= (+ ;[1 2]) 3) "a spliced operand keeps the call")
(assert (< 1 2 3) "a chain in order")
(assert (not (< 1 3 2)) "a chain out of order")
(assert (= 1 1 1) "a chain of equals")
(assert (not (= 1 1 2)) "a chain with one unequal")

# ── A comparison over what is not a number ─────────────────────────

(assert (< "a" "b") "strings order")
(assert (not (< "b" "a")) "strings order the other way")
(assert (< :a :b) "keywords order")
(assert (= "a" "a") "equal strings")
(assert (= 1 1.0) "an int equals its float")
(assert (= [1] [1.0]) "equality is numeric at every depth")
(assert (not (= nil false)) "nil is not false")

# ── A container wrapper keeps the container's own behavior ─────────

(let [s @"a"]
  (push s "b")
  (assert (= s "ab") "push on an @string appends in place"))
(assert (= (push "a" "b") "ab") "push on a string returns a new string")
(assert (= (put {:a 1} :b 2) {:a 1 :b 2}) "put on a struct returns a new struct")
(let [t @{}]
  (put t :x 1)
  (assert (= (get t :x) 1) "put on an @struct stores in place"))

# ── A branch no value reaches goes, and nothing it bound is lost ───

(defn only-strings [b]
  "A branch for an int that only strings ever reach."
  (if (%int? b) (%bit-and b 1) 0))
(assert (= (only-strings "str") 0) "the dead branch is deleted, not rejected")

(defn walk-if-array [x]
  "The :array arm binds a cell for its walk; an int never takes it."
  (match (type-of x)
    :array (each v x
             (println v))
    _ :none))
(assert (= (walk-if-array 1) :none) "deleting the arm orphans nothing")

(defn narrowed [a]
  "y joins an int and a string until the string branch is deleted."
  (let [y (if (%int? a) a "s")]
    (%bit-and y 1)))
(assert (= (narrowed 3) 1) "the narrower join proves the op")

# ── A guard narrows through the wrappers it is written with ────────

(defn low-bit [x]
  "The guard proves x an int through the grafted not and integer?."
  (when (not (integer? x)) (error {:error :type-error :message "low-bit"}))
  (%bit-and x 1))
(def low-bit-value low-bit)
(assert (= (low-bit-value 3) 1) "an int passes the guard")
(assert (= (raises (fn [] (low-bit-value :k))) :type-error) "a keyword raises")
