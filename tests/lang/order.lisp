(elle/epoch 15)
# audited: 2026-11-18
# The one order of all values: what `compare` returns for two values of two kinds, and of one kind.
# docs/types.md
#
# The counter-factual: `compare` raised on an array and on a symbol, ranked a
# string before a keyword, and knew no kind past the keyword. `sort` held an
# order of its own, in which every array came before every @array.
# tests/lang/order-laws.lisp holds the laws, and what follows the order.

(def nan (/ 0.0 0.0))
(def inf (/ 1.0 0.0))
(def f (fn [] 1))
(def fib (fiber/new (fn [] 1) 0))
(def par (parameter 0))
(def bx (box 1))

# ── the sequence of kinds ─────────────────────────────────────────────
# One value of each kind a program builds with no plugin and no FFI, in the
# documented sequence.
(def kinds
  [nil true 1 'a :k "s" () (list 1) [1] b[1] {:a 1} f bx length fib par |1|])

(each i in (range (length kinds))
  (each j in (range (length kinds))
    (let [want (cond
                 (< i j) -1
                 (> i j) 1
                 0)]
      (assert (= want (compare (get kinds i) (get kinds j)))
              (string "kinds order by the documented sequence: " i " against " j)))))

(assert (= kinds (sort (reverse kinds))) "sort puts the kinds in that sequence")
(assert (= ["a" () (list 1)] (sort [(list 1) () "a"]))
        "the empty list sits between a string and a list pair")
(assert (= -1 (compare :a "a")) "a keyword orders before a string")

# ── a kind and its mutable twin share one place ───────────────────────
(def twins
  [[[1 2] @[1 2]] ["ab" @"ab"] [b[1 2] @b[1 2]] [{:a 1} @{:a 1}] [|1 2| @|1 2|]])

(each [fixed mutable] in twins
  (assert (= 0 (compare fixed mutable))
          (string "a value and its mutable twin compare 0: " fixed))
  (assert (= 0 (compare mutable fixed))
          (string "and the same the other way round: " fixed)))

(assert (= -1 (compare @[1] [2])) "an @array orders by its elements")
(assert (= 1 (compare [2] @[1])) "against an array, either way round")
(assert (= 1 (compare @"b" "a")) "an @string orders by its text")
(assert (= -1 (compare @b[1] b[2])) "an @bytes orders by its bytes")
(assert (= -1 (compare @{:a 1} {:a 2})) "an @struct orders by its entries")
(assert (= -1 (compare @|1| |2|)) "an @set orders by its members")

(let [sorted (sort [@[3] [2] @[1] [0]])]
  (assert (= [[0] [1] [2] [3]] sorted)
          "sort orders arrays and @arrays together, by their elements")
  (assert (= [:array :@array :array :@array] (map type-of sorted))
          "and each keeps its own type"))

# ── numbers ───────────────────────────────────────────────────────────
(assert (= -1 (compare 1 2)) "numbers order by value")
(assert (= 0 (compare 1 1.0)) "an integer and a float of one value compare 0")
(assert (= -1 (compare 1 1.5)) "an integer orders against a float by value")
(assert (= -1 (compare -0.0 0.0)) "-0.0 orders before 0.0")
(assert (= 1 (compare 0.0 -0.0)) "and 0.0 after -0.0")
(assert (= -0.0 0.0) "though = calls the two zeros equal")
(assert (= 0 (compare nan nan)) "a NaN compares 0 with itself")
(assert (= 1 (compare nan inf)) "a NaN orders after positive infinity")
(assert (= -1 (compare inf nan)) "and infinity before it")
(assert (= -1 (compare 1 nan)) "an integer orders before a NaN")
(assert (identical? [(- inf) -1 -0.0 0.0 0.5 1 inf nan]
                    (sort [nan inf 1 0.5 0.0 -0.0 -1 (- inf)]))
        "sort puts the numbers in that order, NaN last")

# ── within a kind ─────────────────────────────────────────────────────
(assert (= -1 (compare false true)) "false orders before true")
(assert (= -1 (compare "a" "b")) "strings order by their text")
(assert (= 1 (compare "b" "a")) "either way round")
(assert (= -1 (compare "ab" "abc")) "a string that is a prefix orders first")
(assert (= -1 (compare [1 2] [1 3])) "arrays order element by element")
(assert (= -1 (compare [1] [1 0])) "an array that is a prefix orders first")
(assert (= 1 (compare [2] [1 9])) "the first difference decides")
(assert (= -1 (compare [1 [2 3]] [1 [2 4]]))
        "a nested array orders the same way")
(assert (= -1 (compare [:a] ["a"])) "elements of two kinds order by kind")
(assert (= -1 (compare (list 1 2) (list 1 3))) "lists order element by element")
(assert (= -1 (compare (list 1) (list 1 0)))
        "a list that is a prefix orders first")
(assert (= -1 (compare b[1 2] b[1 3])) "byte sequences order byte by byte")
(assert (= -1 (compare b[1] b[1 0])) "a byte prefix orders first")
(assert (= -1 (compare {:a 1} {:a 2})) "structs order entry by entry")
(assert (= -1 (compare {:a 1} {:a 1 :b 0}))
        "a struct that is a prefix orders first")
(assert (= -1 (compare |1 2| |1 3|)) "sets order member by member")
(assert (= -1 (compare |1| |1 2|)) "a set that is a prefix orders first")

# ── symbols and keywords ──────────────────────────────────────────────
# The order is by name hash, so a test may ask only that it is an order.
(assert (= 0 (compare 'a 'a)) "a symbol compares 0 with itself")
(assert (not= 0 (compare 'a 'b)) "two symbols do not compare 0")
(assert (= (compare 'a 'b) (- (compare 'b 'a))) "and the order is antisymmetric")
(assert (= 0 (compare :a :a)) "a keyword compares 0 with itself")
(assert (not= 0 (compare :a :b)) "two keywords do not compare 0")
(assert (= (compare :a :b) (- (compare :b :a))) "and the order is antisymmetric")
(assert (= (compare :a :b) (if (< :a :b) -1 1))
        "compare and < agree on two keywords")

(println "order: all tests passed")
