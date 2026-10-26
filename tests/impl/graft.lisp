(elle/epoch 15)
# audited: 2026-10-21
# The fold deletes the branches a grafted call's operands cannot take, and a
# guard narrows through the wrappers it is written with.
#
# docs/impl/graft.md
#
# Each function below is called directly, so the graft replaces every call with
# the function's body, and the definition still proves its parameters against
# what those calls passed. The traps are the ones the fold can hit. A branch no
# value reaches must go without taking a binding the rest of the program reads.
# A deletion must narrow the join the branch fed. A guard must still narrow
# through the grafted not and integer? it is written with. What each operator
# answers is a language claim, in tests/lang/operators.lisp.

(defn raises [thunk]
  "The :error kind thunk raises, or nil when it returns."
  (let [[ok? err] (protect (thunk))]
    (if ok? nil (get err :error))))

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

# Both calls are grafted, and the definition proves x against an int and a
# keyword. Only the guard, read through the grafted not and integer?, proves
# the %bit-and there.
(defn low-bit [x]
  "The guard proves x an int through the grafted not and integer?."
  (when (not (integer? x)) (error {:error :type-error :message "low-bit"}))
  (%bit-and x 1))
(assert (= (low-bit 3) 1) "an int passes the guard")
(assert (= (raises (fn [] (low-bit :k))) :type-error) "a keyword raises")
