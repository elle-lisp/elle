(elle/epoch 9)

## ── sort ────────────────────────────────────────────────────────────
(assert (= (sort (list 3 1 2)) (list 1 2 3)) "sort: list")
(assert (= (sort ()) ()) "sort: empty list")
(assert (= (sort (list 1)) (list 1)) "sort: single element")
(assert (= (sort (list 3 1 4 1 5 9 2 6)) (list 1 1 2 3 4 5 6 9))
  "sort: duplicates")
(assert (= (sort (list 1.5 0.5 2.5)) (list 0.5 1.5 2.5)) "sort: floats")
(let [arr @[3 1 2]]
  (let [result (sort arr)]
    (assert (array? result) "sort: array returns array")
    (assert (= (get result 0) 1) "sort: array sorted first")
    (assert (= (get result 1) 2) "sort: array sorted second")
    (assert (= (get result 2) 3) "sort: array sorted third")
    (assert (identical? result arr) "sort: array mutated in place")))
(let [result (sort [3 1 2])]
  (assert (array? result) "sort: array returns array")
  (assert (= (get result 0) 1) "sort: array sorted first")
  (assert (= (get result 1) 2) "sort: array sorted second")
  (assert (= (get result 2) 3) "sort: array sorted third"))

## ── sort: non-numeric types ─────────────────────────────────────────
(assert (= (sort (list "banana" "apple" "cherry"))
    (list "apple" "banana" "cherry")) "sort: strings")
(assert (= (sort (list :b :a :c)) (list :a :b :c)) "sort: keywords")
# Cross-type ordering: nil < bool < int < ... (Value::Ord rank order)
(let [result (sort [nil true 1])]
  (assert (= (get result 0) nil) "sort: cross-type nil first")
  (assert (= (get result 1) true) "sort: cross-type bool second")
  (assert (= (get result 2) 1) "sort: cross-type int third"))

## ── compare ──────────────────────────────────────────────────────────
(assert (= (compare 1 2) -1) "compare: int less")
(assert (= (compare 2 2) 0) "compare: int equal")
(assert (= (compare 3 2) 1) "compare: int greater")
(assert (= (compare "apple" "banana") -1) "compare: string less")
(assert (= (compare "banana" "apple") 1) "compare: string greater")
(assert (= (compare :a :b) -1) "compare: keyword less")
(assert (= (compare nil nil) 0) "compare: nil equal")
(assert (= (compare nil false) -1) "compare: nil rank < bool rank")
(assert (= (compare false true) -1) "compare: false < true")
# Use apply to bypass compile-time arity checking; the runtime arity-error is what we test
(let [[ok? err] (protect ((fn [] (apply compare [1]))))]
  (assert (not ok?) "compare: arity error on 1 arg")
  (assert (= (get err :error) :arity-error) "compare: arity error on 1 arg"))

## ── range ───────────────────────────────────────────────────────────
(let [r (range 5)]
  (assert (array? r) "range: returns array")
  (assert (= (length r) 5) "range: length")
  (assert (= (get r 0) 0) "range: first")
  (assert (= (get r 4) 4) "range: last"))
(let [r (range 2 5)]
  (assert (= (length r) 3) "range: start end length")
  (assert (= (get r 0) 2) "range: start end first")
  (assert (= (get r 2) 4) "range: start end last"))
(let [r (range 0 10 3)]
  (assert (= (length r) 4) "range: with step length")
  (assert (= (get r 0) 0) "range: step first")
  (assert (= (get r 1) 3) "range: step second")
  (assert (= (get r 3) 9) "range: step last"))
(let [r (range 5 0 -1)]
  (assert (= (length r) 5) "range: negative step length")
  (assert (= (get r 0) 5) "range: negative step first")
  (assert (= (get r 4) 1) "range: negative step last"))
(let [r (range 0)]
  (assert (array? r) "range: zero returns array")
  (assert (= (length r) 0) "range: zero is empty"))
(let [r (range 5 5)]
  (assert (= (length r) 0) "range: start=end is empty"))

## ── apply ───────────────────────────────────────────────────────────
(assert (= (apply + (list 1 2 3)) 6) "apply: spread list into +")
(assert (= (apply + 10 (list 1 2 3)) 16) "apply: leading arg + spread")
(assert (= (apply + 10 20 (list 1 2)) 33) "apply: two leading args + spread")
(assert (= (apply list ()) ()) "apply: empty spread")
(assert (= (apply list 1 (list 2 3)) (list 1 2 3)) "apply: mixed args")
(assert (= (apply + @[1 2 3]) 6) "apply: spread array")
(assert (= (apply + [1 2 3]) 6) "apply: spread tuple")

## ── reduce ──────────────────────────────────────────────────────────
(assert (= (reduce + 0 (list 1 2 3)) 6) "reduce: sum")
(assert (= (reduce * 1 (list 2 3 4)) 24) "reduce: product")
(assert (= (reduce + 0 ()) 0) "reduce: empty returns init")

## ── keep ────────────────────────────────────────────────────────────
(assert (= (keep odd? (list 1 2 3 4 5)) (list 1 3 5)) "keep: odd")
(assert (= (keep even? (list 1 2 3 4 5)) (list 2 4)) "keep: even")
(assert (= (keep odd? ()) ()) "keep: empty")

## ── identity ────────────────────────────────────────────────────────
(assert (= (identity 42) 42) "identity: number")
(assert (= (identity nil) nil) "identity: nil")
(assert (= (identity true) true) "identity: bool")
(assert (= (identity (list 1 2)) (list 1 2)) "identity: list")

## ── complement ──────────────────────────────────────────────────────
(assert ((complement nil?) 42) "complement: not nil")
(assert (not ((complement nil?) nil)) "complement: nil")
(assert ((complement even?) 3) "complement: not even")
(assert (not ((complement even?) 4)) "complement: even")

## ── constantly ──────────────────────────────────────────────────────
(assert (= ((constantly 42) 1 2 3) 42) "constantly: ignores args")
(assert (= ((constantly 42)) 42) "constantly: no args")
(assert (= ((constantly nil) :a :b) nil) "constantly: nil")

## ── compose / comp ──────────────────────────────────────────────────
(assert (= ((compose) 42) 42) "compose: zero fns is identity")
(assert (= ((compose (fn (x) (+ x 1))) 5) 6) "compose: single fn")
(assert (= ((compose (fn (x) (* x 2)) (fn (x) (+ x 1))) 3) 8)
  "compose: f(g(x)) = (* 2 (+ 3 1))")
(assert (= ((comp (fn (x) (+ x 1)) (fn (x) (* x 2))) 3) 7)
  "comp: alias, (+ 1 (* 2 3))")

## ── partial ─────────────────────────────────────────────────────────
(assert (= ((partial + 10) 5) 15) "partial: one bound arg")
(assert (= ((partial + 10 20) 5) 35) "partial: two bound args")
(assert (= ((partial * 2 3) 4) 24) "partial: multiply")
(assert (= ((partial list :a) :b :c) (list :a :b :c)) "partial: variadic")

## ── juxt ────────────────────────────────────────────────────────────
(assert (= ((juxt (fn (x) (* x 2)) (fn (x) (+ x 1))) 3) (list 6 4))
  "juxt: two fns")
(assert (= ((juxt even? odd?) 4) (list true false)) "juxt: predicates")

## ── all? ────────────────────────────────────────────────────────────
(assert (all? even? (list 2 4 6)) "all?: all even list")
(assert (not (all? even? (list 2 3 6))) "all?: not all even list")
(assert (all? even? ()) "all?: empty is vacuously true")
(assert (all? even? @[2 4 6]) "all?: array")
(assert (not (all? even? @[2 3 6])) "all?: array not all")
(assert (all? even? [2 4 6]) "all?: tuple")
(assert (not (all? even? [2 3 6])) "all?: tuple not all")

## ── any? ────────────────────────────────────────────────────────────
(assert (any? even? (list 1 2 3)) "any?: has even list")
(assert (not (any? even? (list 1 3 5))) "any?: no even list")
(assert (not (any? even? ())) "any?: empty is false")
(assert (any? even? @[1 2 3]) "any?: array")
(assert (not (any? even? @[1 3 5])) "any?: array none")
(assert (any? even? [1 2 3]) "any?: tuple")
(assert (not (any? even? [1 3 5])) "any?: tuple none")

## ── find ────────────────────────────────────────────────────────────
(assert (= (find even? (list 1 3 4 5)) 4) "find: first even in list")
(assert (= (find even? (list 1 3 5)) nil) "find: not found returns nil")
(assert (= (find even? ()) nil) "find: empty returns nil")
(assert (= (find even? @[1 3 4 5]) 4) "find: array")
(assert (= (find even? [1 3 4 5]) 4) "find: tuple")

## ── find-index ──────────────────────────────────────────────────────
(assert (= (find-index even? (list 1 3 4 5)) 2) "find-index: list")
(assert (= (find-index even? (list 1 3 5)) nil) "find-index: not found")
(assert (= (find-index even? ()) nil) "find-index: empty")
(assert (= (find-index even? @[1 3 4 5]) 2) "find-index: array")
(assert (= (find-index even? [1 3 4 5]) 2) "find-index: tuple")

## ── count ───────────────────────────────────────────────────────────
(assert (= (count even? (list 1 2 3 4 5 6)) 3) "count: list")
(assert (= (count even? ()) 0) "count: empty")
(assert (= (count even? @[1 2 3 4 5 6]) 3) "count: array")
(assert (= (count even? [1 2 3 4 5 6]) 3) "count: tuple")

## ── nth ─────────────────────────────────────────────────────────────
(assert (= (nth 0 (list 10 20 30)) 10) "nth: first of list")
(assert (= (nth 2 (list 10 20 30)) 30) "nth: last of list")
(assert (= (nth 1 @[10 20 30]) 20) "nth: array")
(assert (= (nth 1 [10 20 30]) 20) "nth: tuple")

## ── zip ─────────────────────────────────────────────────────────────
(let [z (zip (list 1 2 3) (list :a :b :c))]
  (assert (= (length z) 3) "zip: length")
  (assert (= (first z) (list 1 :a)) "zip: first pair")
  (assert (= (first (rest z)) (list 2 :b)) "zip: second pair"))
(let [z (zip (list 1 2) (list :a :b :c))]
  (assert (= (length z) 2) "zip: stops at shortest"))
(assert (= (zip) ()) "zip: no args")
(let [z (zip @[1 2 3] @[:a :b :c])]
  (assert (array? z) "zip: array input returns array")
  (assert (= (length z) 3) "zip: array length"))
(let [z (zip [1 2] [:a :b])]
  (assert (array? z) "zip: array input returns array"))

## ── flatten ─────────────────────────────────────────────────────────
(assert (= (flatten (list 1 (list 2 3) (list 4 (list 5)))) (list 1 2 3 4 5))
  "flatten: nested lists")
(assert (= (flatten ()) ()) "flatten: empty")
(assert (= (flatten (list 1 2 3)) (list 1 2 3)) "flatten: already flat")
(let [f (flatten @[1 @[2 3] @[4]])]
  (assert (array? f) "flatten: array returns array")
  (assert (= (length f) 4) "flatten: array length"))

## ── take-while ──────────────────────────────────────────────────────
(assert (= (take-while even? (list 2 4 5 6)) (list 2 4)) "take-while: list")
(assert (= (take-while even? (list 1 2 3)) ()) "take-while: none match")
(assert (= (take-while even? ()) ()) "take-while: empty")
(let [tw (take-while even? @[2 4 5 6])]
  (assert (array? tw) "take-while: array returns array")
  (assert (= (length tw) 2) "take-while: array length"))
(let [tw (take-while even? [2 4 5 6])]
  (assert (array? tw) "take-while: array returns array"))

## ── drop-while ──────────────────────────────────────────────────────
(assert (= (drop-while even? (list 2 4 5 6)) (list 5 6)) "drop-while: list")
(assert (= (drop-while even? (list 1 2 3)) (list 1 2 3))
  "drop-while: none dropped")
(assert (= (drop-while even? ()) ()) "drop-while: empty")
(let [dw (drop-while even? @[2 4 5 6])]
  (assert (array? dw) "drop-while: array returns array")
  (assert (= (length dw) 2) "drop-while: array length"))
(let [dw (drop-while even? [2 4 5 6])]
  (assert (array? dw) "drop-while: array returns array"))

## ── distinct ────────────────────────────────────────────────────────
(assert (= (distinct (list 1 2 1 3 2 4)) (list 1 2 3 4)) "distinct: list")
(assert (= (distinct ()) ()) "distinct: empty")
(let [d (distinct @[1 2 1 3 2 4])]
  (assert (array? d) "distinct: array returns array")
  (assert (= (length d) 4) "distinct: array deduped"))
(let [d (distinct [1 2 1 3 2 4])]
  (assert (array? d) "distinct: array returns array"))

## ── frequencies ─────────────────────────────────────────────────────
(let [freq (frequencies (list :a :b :a :c :b :a))]
  (assert (struct? freq) "frequencies: returns struct")
  (assert (= (get freq :a) 3) "frequencies: a=3")
  (assert (= (get freq :b) 2) "frequencies: b=2")
  (assert (= (get freq :c) 1) "frequencies: c=1"))
(let [freq (frequencies @[:a :b :a])]
  (assert (struct? freq) "frequencies: array input")
  (assert (= (get freq :a) 2) "frequencies: array a=2"))
(let [freq (frequencies ())]
  (assert (struct? freq) "frequencies: empty"))

## ── mapcat ──────────────────────────────────────────────────────────
(assert (= (mapcat (fn (x) (list x (* x 10))) (list 1 2 3))
    (list 1 10 2 20 3 30)) "mapcat: list")
(assert (= (mapcat (fn (x) ()) (list 1 2 3)) ()) "mapcat: empty results")
(let [mc (mapcat (fn (x) @[x (* x 10)]) @[1 2 3])]
  (assert (array? mc) "mapcat: array returns array")
  (assert (= (length mc) 6) "mapcat: array length"))

## ── group-by ────────────────────────────────────────────────────────
(let [groups (group-by even? (list 1 2 3 4 5 6))]
  (assert (struct? groups) "group-by: returns table")
  (assert (= (length (get groups true)) 3) "group-by: evens count")
  (assert (= (length (get groups false)) 3) "group-by: odds count"))
(let [groups (group-by even? @[1 2 3 4 5 6])]
  (assert (struct? groups) "group-by: array input")
  (assert (= (length (get groups true)) 3) "group-by: array evens"))

## ── map-indexed ─────────────────────────────────────────────────────
(assert (= (map-indexed (fn (i x) (list i x)) (list :a :b :c))
    (list (list 0 :a) (list 1 :b) (list 2 :c))) "map-indexed: list")
(assert (= (map-indexed (fn (i x) (list i x)) ()) ()) "map-indexed: empty")
(let [mi (map-indexed (fn (i x) (+ i x)) @[10 20 30])]
  (assert (array? mi) "map-indexed: array returns array")
  (assert (= (get mi 0) 10) "map-indexed: 0+10")
  (assert (= (get mi 1) 21) "map-indexed: 1+20")
  (assert (= (get mi 2) 32) "map-indexed: 2+30"))

## ── partition ───────────────────────────────────────────────────────
(let [p (partition 2 (list 1 2 3 4 5))]
  (assert (= (length p) 3) "partition: list count")
  (assert (= (first p) (list 1 2)) "partition: first group")
  (assert (= (last p) (list 5)) "partition: last group partial"))
(let [p (partition 2 @[1 2 3 4 5])]
  (assert (array? p) "partition: array returns array")
  (assert (= (length p) 3) "partition: array count")
  (assert (array? (get p 0)) "partition: array chunks are arrays"))
(assert (= (partition 3 ()) ()) "partition: empty")

## ── interpose ───────────────────────────────────────────────────────
(assert (= (interpose :sep (list 1 2 3)) (list 1 :sep 2 :sep 3))
  "interpose: list")
(assert (= (interpose :sep (list 1)) (list 1)) "interpose: single")
(assert (= (interpose :sep ()) ()) "interpose: empty")
(let [ip (interpose :sep @[1 2 3])]
  (assert (array? ip) "interpose: array returns array")
  (assert (= (length ip) 5) "interpose: array length"))

## ── min-key / max-key ───────────────────────────────────────────────
(assert (= (min-key abs -3 1 -7 4) 1) "min-key: smallest abs")
(assert (= (max-key abs -3 1 -7 4) -7) "max-key: largest abs")
(assert (= (min-key identity 5 3 8 1) 1) "min-key: identity")
(assert (= (max-key identity 5 3 8 1) 8) "max-key: identity")

## ── memoize ─────────────────────────────────────────────────────────
(let* [@call-count 0
       mf (memoize (fn (x)
                     (assign call-count (+ call-count 1))
                     (* x x)))]
  (assert (= (mf 3) 9) "memoize: compute 3*3")
  (assert (= (mf 3) 9) "memoize: cached 3*3")
  (assert (= (mf 4) 16) "memoize: compute 4*4")
  (assert (= call-count 2) "memoize: called twice not thrice"))

## ── sort-by ─────────────────────────────────────────────────────────
(assert (= (sort-by abs (list -3 1 -2)) (list 1 -2 -3)) "sort-by: abs list")
(assert (= (sort-by identity (list 3 1 2)) (list 1 2 3))
  "sort-by: identity = sort")
(assert (= (sort-by identity ()) ()) "sort-by: empty")
(let [result (sort-by abs @[-3 1 -2])]
  (assert (array? result) "sort-by: array returns array")
  (assert (= (get result 0) 1) "sort-by: array first")
  (assert (= (get result 1) -2) "sort-by: array second")
  (assert (= (get result 2) -3) "sort-by: array third"))
(let [result (sort-by abs [3 1 2])]
  (assert (array? result) "sort-by: array returns array"))

## ── sort-with ────────────────────────────────────────────────────────
# Basic list sort using compare
(assert (= (sort-with compare (list 3 1 2)) (list 1 2 3))
  "sort-with: basic list ascending")
# Descending list sort
(assert (= (sort-with (fn (a b) (compare b a)) (list 3 1 2)) (list 3 2 1))
  "sort-with: descending")
# String sort
(assert (= (sort-with compare (list "banana" "apple" "cherry"))
    (list "apple" "banana" "cherry")) "sort-with: strings")
# Empty list
(assert (= (sort-with compare ()) ()) "sort-with: empty")
# Single element
(assert (= (sort-with compare (list 42)) (list 42)) "sort-with: single")
# Ascending with subtraction comparator
(assert (= (sort-with (fn (a b) (- a b)) (list 3 1 2)) (list 1 2 3))
  "sort-with: ascending subtraction")
# Mutable array sorted returns new mutable array
(let [orig @[1 3 2]
      result (sort-with (fn (a b) (compare b a)) @[1 3 2])]
  (assert (mutable? result) "sort-with: @array returns mutable")
  (assert (= (get result 0) 3) "sort-with: @array first")
  (assert (= (get result 1) 2) "sort-with: @array second")
  (assert (= (get result 2) 1) "sort-with: @array third"))
# Immutable array returns new immutable array
(let [result (sort-with (fn (a b) (- a b)) [3 1 2])]
  (assert (array? result) "sort-with: array returns array")
  (assert (not (mutable? result)) "sort-with: array returns immutable")
  (assert (= (get result 0) 1) "sort-with: array first"))
# Stability: equal elements preserve insertion order
(assert (= (sort-with (fn (a b) 0) (list 3 1 2)) (list 3 1 2))
  "sort-with: all-equal preserves order")
# sort-by-cmp alias works
(assert (= (sort-by-cmp compare (list 3 1 2)) (list 1 2 3))
  "sort-by-cmp: alias works")

## ── freeze / thaw: structs ───────────────────────────────────────────
(let [t @{:a 1 :b 2}]
  (let [s (freeze t)]
    (assert (struct? s) "freeze: returns struct")
    (assert (= (get s :a) 1) "freeze: preserves values")
    (assert (= (get s :b) 2) "freeze: preserves all keys")))

(let [s {:a 1 :b 2}]
  (let [t (thaw s)]
    (assert (struct? t) "thaw: returns @struct")
    (assert (= (get t :a) 1) "thaw: preserves values")
    (put t :c 3)
    (assert (= (get t :c) 3) "thaw: result is mutable")))

## ── freeze / thaw: arrays ───────────────────────────────────────────
(let [ma @[1 2 3]]
  (let [a (freeze ma)]
    (assert (array? a) "freeze @array: returns array")
    (assert (= (get a 0) 1) "freeze @array: preserves values")
    (assert (= (length a) 3) "freeze @array: preserves length")))

(let [a [1 2 3]]
  (let [ma (thaw a)]
    (assert (= (type-of ma) :@array) "thaw array: returns @array")
    (assert (= (get ma 0) 1) "thaw array: preserves values")
    (push ma 4)
    (assert (= (length ma) 4) "thaw array: result is mutable")))

(assert (= (freeze [1 2 3]) [1 2 3]) "freeze: immutable array returns as-is")
(assert (= (type-of (thaw (thaw [1 2 3]))) :@array) "thaw: idempotent on @array")

## ── freeze / thaw: strings ──────────────────────────────────────────
(assert (= (freeze (thaw "hello")) "hello") "freeze @string: returns string")
(assert (= (type-of (freeze (thaw "hello"))) :string)
  "freeze @string: type is string")
(assert (= (freeze "hello") "hello") "freeze: immutable string returns as-is")

(assert (= (type-of (thaw "hello")) :@string) "thaw string: returns @string")
(assert (= (freeze (thaw "hello")) "hello") "thaw then freeze: roundtrip")
(assert (= (type-of (thaw (thaw "hello"))) :@string)
  "thaw: idempotent on @string")

## ── freeze / thaw: bytes ────────────────────────────────────────────
(let [mb (@bytes 1 2 3)]
  (let [b (freeze mb)]
    (assert (bytes? b) "freeze @bytes: returns bytes")
    (assert (= (get b 0) 1) "freeze @bytes: preserves values")))

(let [b (bytes 1 2 3)]
  (let [mb (thaw b)]
    (assert (= (type-of mb) :@bytes) "thaw bytes: returns @bytes")
    (assert (= (get mb 0) 1) "thaw bytes: preserves values")))

(assert (= (type-of (freeze (freeze (@bytes 1 2 3)))) :bytes)
  "freeze: idempotent on bytes")
(assert (= (type-of (thaw (thaw (bytes 1 2 3)))) :@bytes)
  "thaw: idempotent on @bytes")

## ── freeze @string: invalid UTF-8 ──────────────────────────────────
(let [[ok? _] (protect ((fn [] (freeze (@string 255 254)))))]
  (assert (not ok?) "freeze @string with invalid UTF-8 signals error"))
