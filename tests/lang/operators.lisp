(elle/epoch 15)
# audited: 2026-10-21
# Each arithmetic and comparison operator answers by IEEE 754 and 64-bit wrap,
# at every arity, and raises over an operand it refuses.
#
# Every operand pair is checked three ways: written as literals in the call,
# read from a table at run time, and through the operator called as a value. An
# implementation may compile the three differently, and none may change an
# answer.
#
# The counter-factual: a + that summed from the integer 0 would return 0.0 for
# (+ -0.0 -0.0), because 0 + -0.0 is 0.0. IEEE 754 keeps the negative zero, and
# so does a call compiled to one addition, so the answer would turn on how a
# call compiled.

(def nan (/ 0.0 0.0))
(def inf (math/inf))

(defn raises [thunk]
  "The :error kind thunk raises, or nil when it returns."
  (let [[ok? err] (protect (thunk))]
    (if ok? nil (get err :error))))

(defn same? [got want]
  "Whether got answers want: any NaN for a NaN, else the same type and bits."
  (if (nan? want) (nan? got) (identical? got want)))

(def names ["+" "-" "*" "<" ">" "<=" ">=" "="])

(defn agree [spelling a b got wants]
  "Assert that each answer in got is its peer in wants."
  (each i (range (length wants))
    (assert (same? (get got i) (get wants i))
            (string "(" (get names i) " " a " " b ") " spelling ": got "
                    (get got i) ", want " (get wants i)))))

# ── Every operand pair, three ways ─────────────────────────────────

(def table @[])

# Check a and b with the operands written in each call, and keep the pair for
# the two spellings below. wants is the answer of +, -, *, <, >, <=, >= and =.
(defmacro row (a b & wants)
  `(begin
     (agree "over literals" ,a ,b
            [(+ ,a ,b) (- ,a ,b) (* ,a ,b) (< ,a ,b) (> ,a ,b) (<= ,a ,b)
             (>= ,a ,b) (= ,a ,b)] (array ,;wants))
     (push table [,a ,b (array ,;wants)])))

(row 1 2 3 -1 2 true false true false false)
(row -7 4 -3 -11 -28 true false true false false)
(row 0 0 0 0 0 false false true true true)
(row 9223372036854775807 1 -9223372036854775808 9223372036854775806
     9223372036854775807 false true false true false)
(row -9223372036854775808 -1 9223372036854775807 -9223372036854775807
     -9223372036854775808 true false true false false)
(row 1.5 2 3.5 -0.5 3.0 true false true false false)
(row 2 1.5 3.5 0.5 3.0 false true false true false)
(row 1 1.0 2.0 0.0 1.0 false false true true true)
(row -0.0 -0.0 -0.0 0.0 0.0 false false true true true)
(row -0.0 0.0 0.0 -0.0 -0.0 false false true true true)
(row 0.0 -0.0 0.0 0.0 -0.0 false false true true true)
(row 0 -0.0 0.0 0.0 -0.0 false false true true true)
(row -0.0 0 0.0 -0.0 -0.0 false false true true true)
(row nan 1.0 nan nan nan false false false false false)
(row 1 nan nan nan nan false false false false false)
(row nan nan nan nan nan false false false false false)
(row inf (- inf) nan inf (- inf) false true false true false)
(row inf 1 inf inf inf false true false true false)

(defn read-answers [a b]
  "Each operator over two operands read from the table."
  [(+ a b) (- a b) (* a b) (< a b) (> a b) (<= a b) (>= a b) (= a b)])

(def operators [+ - * < > <= >= =])

(each [a b wants] table
  (agree "over operands read from a table" a b (read-answers a b) wants)
  (agree "called as values" a b (map (fn [op] (op a b)) operators) wants))

# ── Every arity ────────────────────────────────────────────────────

(assert (identical? (+) 0) "the empty sum is the integer 0")
(assert (identical? (*) 1) "the empty product is the integer 1")
(assert (identical? (+ -0.0) -0.0) "one operand is its own sum")
(assert (= (- 5) -5) "one operand negates")
(assert (= (+ 1 2 3) 6) "three operands sum")
(assert (identical? (+ -0.0 -0.0 -0.0) -0.0)
        "three negative zeros sum to a negative zero")
(assert (= (- 10 1 2) 7) "subtraction runs left to right")
(assert (= (* 2 3 4) 24) "three operands multiply")
(assert (= (+ ;[1 2]) 3) "spliced operands sum")
(assert (< 1 2 3) "a chain in order")
(assert (not (< 1 3 2)) "a chain out of order")
(assert (> 3 2 1) "a descending chain")
(assert (<= 1 1 2) "a chain that holds an equal pair")
(assert (>= 2 2 1) "a descending chain that holds an equal pair")
(assert (= 1 1 1) "a chain of equals")
(assert (not (= 1 1 2)) "a chain with one unequal")

# ── An operand the operator refuses ────────────────────────────────

(assert (= (raises (fn [] (+ "a" 1))) :type-error) "a string is not a summand")
(assert (= (raises (fn [] (+ 1 "a"))) :type-error)
        "the second operand is checked too")
(assert (= (raises (fn [] (+ 1 2 "c"))) :type-error)
        "three operands are each checked")
(assert (= (raises (fn [] (- :k))) :type-error) "a keyword does not negate")
(assert (= (raises (fn [] (< 1 "a"))) :type-error)
        "an int and a string do not compare")
(each x ["s" :k nil]
  (assert (= (raises (fn [] (+ x))) :type-error)
          (string "one operand that is not a number raises: " x)))
(assert (= (raises (fn [] (put {} :a 1 2))) :arity-error)
        "put takes at most three operands")

# ── Equality over what is not a number ────────────────────────────

(assert (= "a" "a") "equal strings")
(assert (= [1] [1.0]) "equality is numeric at every depth")
(assert (not (= nil false)) "nil is not false")
(assert (not (= 1 "1")) "an int is not its spelling")
