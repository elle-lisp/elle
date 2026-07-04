(elle/epoch 12)
# Numeric correctness tests
#
# Mixed int/float arithmetic, comparisons, overflow handling,
# IEEE 754 special values, and hash consistency.

# ── Mixed int/float arithmetic ──────────────────────────────────────────
(assert (= (+ 1 0.5) 1.5) "+ int float")
(assert (= (- 1 0.5) 0.5) "- int float")
(assert (= (* 2 0.5) 1.0) "* int float")
(assert (= (/ 1.0 2) 0.5) "/ float int")
(assert (= (/ 7.0 2) 3.5) "/ float int non-even")

# ── Integer division truncates ──────────────────────────────────────────
(assert (= (/ 5 3) 1) "int div truncates")
(assert (= (/ -5 3) -1) "int div truncates negative")

# ── Float display preserves type ────────────────────────────────────────
(assert (= (string 3.0) "3.0") "float string keeps .0")
(assert (= (string 3.14) "3.14") "float string keeps decimals")
(assert (= (string 0.0) "0.0") "zero float string")

# ── IEEE 754 division ──────────────────────────────────────────────────
(assert (inf? (/ 1.0 0.0)) "float/0 = inf")
(assert (inf? (/ 1.0 0)) "float/int0 = inf")
(assert (inf? (/ -1.0 0.0)) "-float/0 = -inf")
(assert (nan? (/ 0.0 0.0)) "0.0/0.0 = NaN")
(def [ok _] (protect (/ 1 0)))
(assert (not ok) "int/0 errors")

# ── IEEE 754 constants ──────────────────────────────────────────────────
(assert (inf? (+inf)) "+inf is infinite")
(assert (inf? (-inf)) "-inf is infinite")
(assert (nan? (nan)) "nan is NaN")
(assert (= (string (+inf)) "inf") "+inf displays as inf")
(assert (= (string (-inf)) "-inf") "-inf displays as -inf")
(assert (= (string (nan)) "NaN") "nan displays as NaN")

# ── min / max mixed ────────────────────────────────────────────────────
(assert (= (min 3 2.5) 2.5) "min int float")
(assert (= (max 3 2.5) 3) "max int float")
(assert (= (min 0.5 1 2) 0.5) "min variadic mixed")
(assert (= (max 0.5 1 2) 2) "max variadic mixed")

# ── Hash consistency ────────────────────────────────────────────────────
(assert (= (hash 1) (hash 1.0)) "hash 1 = hash 1.0")
(assert (= (hash 0) (hash 0.0)) "hash 0 = hash 0.0")
(assert (= (hash -1) (hash -1.0)) "hash -1 = hash -1.0")

# ── Sort mixed ──────────────────────────────────────────────────────────
(assert (= (sort [2 0.5 1 1.5]) [0.5 1 1.5 2]) "sort mixed")

# ── pow mixed ───────────────────────────────────────────────────────────
(assert (= (pow 2 -1) 0.5) "pow int neg exp")
(assert (= (pow 2.0 -1) 0.5) "pow float neg exp")
(assert (= (pow 2.0 3) 8.0) "pow float int")
(assert (= (pow 0 0) 1) "pow 0 0")

# ── Integer overflow ───────────────────────────────────────────────────
# Default mode: %-intrinsics use wrapping arithmetic (WASM/SPIR-V semantics).
# --checked-intrinsics: NativeFn path uses checked_add → overflow errors.
(def checked? (vm/config :checked-intrinsics))
(if checked?
  (begin
    (def [add-ok _] (protect (+ 9223372036854775807 1)))
    (assert (not add-ok) "int add overflow errors (checked)")
    (def [sub-ok _] (protect (- -9223372036854775808 1)))
    (assert (not sub-ok) "int sub overflow errors (checked)")
    (def [mul-ok _] (protect (* 9223372036854775807 2)))
    (assert (not mul-ok) "int mul overflow errors (checked)"))
  (begin
    (assert (= (+ 9223372036854775807 1) -9223372036854775808)
            "int add overflow wraps")
    (assert (= (- -9223372036854775808 1) 9223372036854775807)
            "int sub overflow wraps")
    (assert (= (* 9223372036854775807 2) -2) "int mul overflow wraps")))

# ── NaN comparisons ─────────────────────────────────────────────────────
# Note: (= nan nan) is true in Elle (structural equality, not IEEE 754).
# Ordering comparisons correctly reject NaN.
(assert (not (< (nan) 1)) "NaN not < 1")
(assert (not (< 1 (nan))) "1 not < NaN")
(assert (not (> (nan) 1)) "NaN not > 1")

# ── even? / odd? ──────────────────────────────────────────────────────
(assert (even? 0) "even? 0")
(assert (even? 2) "even? 2")
(assert (even? -4) "even? -4")
(assert (not (even? 1)) "even? 1")
(assert (not (even? -3)) "even? -3")
(assert (odd? 1) "odd? 1")
(assert (odd? -3) "odd? -3")
(assert (not (odd? 0)) "odd? 0")
(assert (not (odd? 2)) "odd? 2")
(def [ok _] (protect (even? 1.5)))
(assert (not ok) "even? rejects float")
(def [ok _] (protect (even? "x")))
(assert (not ok) "even? rejects string")

# ── abs ───────────────────────────────────────────────────────────────
(assert (= (abs 5) 5) "abs pos int")
(assert (= (abs -5) 5) "abs neg int")
(assert (= (abs 0) 0) "abs zero")
(assert (= (abs 3.5) 3.5) "abs pos float")
(assert (= (abs -3.5) 3.5) "abs neg float")
(assert (= (abs 0.0) 0.0) "abs zero float")
(def [ok _] (protect (abs "x")))
(assert (not ok) "abs rejects string")
# abs of i64::MIN should overflow
(def [ok _] (protect (abs -9223372036854775808)))
(assert (not ok) "abs i64::MIN overflows")

# ── floor ─────────────────────────────────────────────────────────────
(assert (= (floor 3) 3) "floor int passthrough")
(assert (= (floor -3) -3) "floor neg int passthrough")
(assert (= (floor 3.0) 3) "floor exact float")
(assert (= (floor 3.7) 3) "floor pos float")
(assert (= (floor 3.2) 3) "floor pos float low")
(assert (= (floor -3.2) -4) "floor neg float")
(assert (= (floor -3.7) -4) "floor neg float high")
(assert (= (floor -3.0) -3) "floor neg exact float")
(assert (= (floor 0.5) 0) "floor 0.5")
(assert (= (floor -0.5) -1) "floor -0.5")
(def [ok _] (protect (floor "x")))
(assert (not ok) "floor rejects string")

# ── ceil ──────────────────────────────────────────────────────────────
(assert (= (ceil 3) 3) "ceil int passthrough")
(assert (= (ceil -3) -3) "ceil neg int passthrough")
(assert (= (ceil 3.0) 3) "ceil exact float")
(assert (= (ceil 3.2) 4) "ceil pos float")
(assert (= (ceil 3.7) 4) "ceil pos float high")
(assert (= (ceil -3.2) -3) "ceil neg float")
(assert (= (ceil -3.7) -3) "ceil neg float high")
(assert (= (ceil -3.0) -3) "ceil neg exact float")
(assert (= (ceil 0.5) 1) "ceil 0.5")
(assert (= (ceil -0.5) 0) "ceil -0.5")
(def [ok _] (protect (ceil "x")))
(assert (not ok) "ceil rejects string")

# ── round ─────────────────────────────────────────────────────────────
(assert (= (round 3) 3) "round int passthrough")
(assert (= (round 3.4) 3) "round down")
(assert (= (round 3.5) 4) "round half up")
(assert (= (round 3.6) 4) "round up")
(assert (= (round -3.4) -3) "round neg down")
(assert (= (round -3.5) -4) "round neg half away")
(assert (= (round -3.6) -4) "round neg up")
(assert (= (round 0.5) 1) "round 0.5")
(assert (= (round -0.5) -1) "round -0.5")
(def [ok _] (protect (round "x")))
(assert (not ok) "round rejects string")

# ── not= ──────────────────────────────────────────────────────────────
(assert (not= 1 2) "not= different ints")
(assert (not (not= 1 1)) "not= equal ints")
(assert (not (not= 1 1.0)) "not= int float coercion")
(assert (not= "a" "b") "not= different strings")
(assert (not (not= "a" "a")) "not= equal strings")

# ── nonempty? ─────────────────────────────────────────────────────────
(assert (nonempty? [1]) "nonempty? array")
(assert (not (nonempty? [])) "nonempty? empty array")
(assert (nonempty? "x") "nonempty? string")
(assert (not (nonempty? "")) "nonempty? empty string")
(assert (nonempty? {:a 1}) "nonempty? struct")
(assert (not (nonempty? {})) "nonempty? empty struct")

# ── min / max ─────────────────────────────────────────────────────────
(assert (= (min 3) 3) "min single")
(assert (= (min 3 1 4 1 5) 1) "min variadic")
(assert (= (min -5 -3 -1) -5) "min negative")
(assert (= (min 1 0.5) 0.5) "min int float")
(assert (= (max 3) 3) "max single")
(assert (= (max 3 1 4 1 5) 5) "max variadic")
(assert (= (max -5 -3 -1) -1) "max negative")
(assert (= (max 1 0.5) 1) "max int float")
(def [ok _] (protect (min "a" 1)))
(assert (not ok) "min rejects non-number")
(def [ok _] (protect (max 1 "a")))
(assert (not ok) "max rejects non-number")

# ── compare ───────────────────────────────────────────────────────────
(assert (= (compare 1 2) -1) "compare less")
(assert (= (compare 2 2) 0) "compare equal")
(assert (= (compare 3 2) 1) "compare greater")
(assert (= (compare "a" "b") -1) "compare strings")
(assert (= (compare :x :x) 0) "compare keywords")

# ── range ─────────────────────────────────────────────────────────────
(assert (= (range 5) [0 1 2 3 4]) "range end")
(assert (= (range 2 5) [2 3 4]) "range start end")
(assert (= (range 0 10 3) [0 3 6 9]) "range step")
(assert (= (range 5 0 -1) [5 4 3 2 1]) "range negative step")
(assert (= (range 0) []) "range empty")
(assert (= (range 5 5) []) "range start=end")
(def [ok _] (protect (range 0 10 0)))
(assert (not ok) "range zero step")

# ── assert ────────────────────────────────────────────────────────────
(assert true "assert true")
(assert 1 "assert truthy int")
(assert "x" "assert truthy string")
(def [ok err] (protect (assert false "boom")))
(assert (not ok) "assert false signals")
(assert (= err:error :failed-assertion) "assert error type")
(assert (= err:message "boom") "assert error message")
(def [ok err] (protect (assert nil)))
(assert (not ok) "assert nil signals")
(assert (= err:error :failed-assertion) "assert nil error type")

# ── xor ───────────────────────────────────────────────────────────────
(assert (xor true) "xor single true")
(assert (not (xor false)) "xor single false")
(assert (xor true false) "xor true false")
(assert (not (xor true true)) "xor true true")
(assert (not (xor false false)) "xor false false")
(assert (not (xor true false true false)) "xor even truthy 4-arg")
(assert (xor true true true false) "xor odd truthy 4-arg")
(assert (not (xor)) "xor empty")

# ── last ──────────────────────────────────────────────────────────────
(assert (= (last [1 2 3]) 3) "last array")
(assert (= (last (list 1 2 3)) 3) "last list")
(assert (= (last "abc") "c") "last string")
(def [ok _] (protect (last [])))
(assert (not ok) "last empty array errors")
(def [ok _] (protect (last 42)))
(assert (not ok) "last rejects non-sequence")

# ── butlast ───────────────────────────────────────────────────────────
(assert (= (butlast [1 2 3]) [1 2]) "butlast array")
(assert (= (butlast (list 1 2 3)) (list 1 2)) "butlast list")
(assert (= (butlast [1]) []) "butlast single")
(assert (= (butlast []) []) "butlast empty")

# ── take ──────────────────────────────────────────────────────────────
(assert (= (take 2 (list 1 2 3)) (list 1 2)) "take 2")
(assert (= (take 0 (list 1 2 3)) ()) "take 0")
(assert (= (take 5 (list 1 2 3)) (list 1 2 3)) "take more than length")
(assert (= (take 0 ()) ()) "take 0 empty")
(def [ok _] (protect (take -1 (list 1 2))))
(assert (not ok) "take negative errors")

# ── drop ──────────────────────────────────────────────────────────────
(assert (= (drop 1 (list 1 2 3)) (list 2 3)) "drop 1")
(assert (= (drop 0 (list 1 2 3)) (list 1 2 3)) "drop 0")
(assert (= (drop 5 (list 1 2 3)) ()) "drop more than length")
(assert (= (drop 0 ()) ()) "drop 0 empty")
(def [ok _] (protect (drop -1 (list 1 2))))
(assert (not ok) "drop negative errors")

# ── @string constructor with string args ─────────────────────────────
(assert (= (string (@string "hello")) "hello") "@string from string")
(assert (= (string (@string "a" "b" "c")) "abc") "@string concat strings")
(assert (= (string (@string "hi" 33)) "hi!") "@string mixed string+byte")
(assert (= (length (@string)) 0) "@string empty")

# ── reverse ──────────────────────────────────────────────────────────
(assert (= (reverse (list 1 2 3)) (list 3 2 1)) "reverse list")
(assert (= (reverse ()) ()) "reverse empty list")
(assert (= (reverse [1 2 3]) [3 2 1]) "reverse array")
(assert (= (reverse []) []) "reverse empty array")
(assert (= (reverse "abc") "cba") "reverse string")
(assert (= (reverse "") "") "reverse empty string")
(assert (= (reverse (bytes 1 2 3)) (bytes 3 2 1)) "reverse bytes")
(assert (= (reverse (bytes)) (bytes)) "reverse empty bytes")
(let [ma (@array 1 2 3)]
  (def rev-ma (reverse ma))
  (assert (= rev-ma @[3 2 1]) "reverse @array"))
(let [ms (@string 97 98 99)]
  (def rev-ms (reverse ms))
  (assert (= (string rev-ms) "cba") "reverse @string"))
(let [mb (@bytes 1 2 3)]
  (def rev-mb (reverse mb))
  (assert (= rev-mb (@bytes 3 2 1)) "reverse @bytes"))
(def [ok _] (protect (reverse |1 2 3|)))
(assert (not ok) "reverse rejects set")
(def [ok _] (protect (reverse {:a 1})))
(assert (not ok) "reverse rejects struct")

# ── append ───────────────────────────────────────────────────────────
(assert (= (append (list 1 2) (list 3 4)) (list 1 2 3 4)) "append lists")
(assert (= (append () (list 1)) (list 1)) "append empty list left")
(assert (= (append (list 1) ()) (list 1)) "append empty list right")
(assert (= (append [1 2] [3 4]) [1 2 3 4]) "append arrays")
(assert (= (append [] [1]) [1]) "append empty array left")
(assert (= (append "ab" "cd") "abcd") "append strings")
(assert (= (append "" "x") "x") "append empty string left")
(assert (= (append (bytes 1 2) (bytes 3 4)) (bytes 1 2 3 4)) "append bytes")
(let [ma @[1 2]]
  (append ma @[3 4])
  (assert (= ma @[1 2 3 4]) "append @array mutates"))
(let [ms (@string 97 98)]
  (append ms (@string 99 100))
  (assert (= (string ms) "abcd") "append @string mutates"))
(let [mb (@bytes 1 2)]
  (append mb (@bytes 3 4))
  (assert (= mb (@bytes 1 2 3 4)) "append @bytes mutates"))
(assert (= (append |1 2| |2 3|) |1 2 3|) "append sets = union")
(assert (= (append {:a 1} {:b 2}) {:a 1 :b 2}) "append structs = merge")
(def [ok _] (protect (append 42 43)))
(assert (not ok) "append rejects non-collection")

# ── concat ───────────────────────────────────────────────────────────
(assert (= (concat [1 2]) [1 2]) "concat single arg")
(assert (= (concat [1 2] [3] [4 5]) [1 2 3 4 5]) "concat arrays")
(assert (= (concat "a" "b" "c") "abc") "concat strings")
(assert (= (concat (list 1) (list 2) (list 3)) (list 1 2 3)) "concat lists")

# ── fold / reduce ───────────────────────────────────────────────────
(assert (= (fold + 0 [1 2 3]) 6) "fold sum")
(assert (= (fold + 0 ()) 0) "fold empty")
(assert (= (reduce + 0 [1 2 3]) 6) "reduce sum (3-arg alias for fold)")
(assert (= (reduce + 0 ()) 0) "reduce empty returns init")

# ── quasiquote splice ───────────────────────────────────────────────
(defmacro splice-test [& items]
  `(list ,;items))
(assert (= (splice-test 10 20 30) (list 10 20 30)) "quasiquote splice")
