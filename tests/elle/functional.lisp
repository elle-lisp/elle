(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## ── sort ────────────────────────────────────────────────────────────
(assert-list-eq (sort (list 3 1 2)) (list 1 2 3) "sort: list")
(assert-list-eq (sort ()) () "sort: empty list")
(assert-list-eq (sort (list 1)) (list 1) "sort: single element")
(assert-list-eq (sort (list 3 1 4 1 5 9 2 6)) (list 1 1 2 3 4 5 6 9) "sort: duplicates")
(assert-list-eq (sort (list 1.5 0.5 2.5)) (list 0.5 1.5 2.5) "sort: floats")
(let ((arr @[3 1 2]))
  (let ((result (sort arr)))
    (assert-true (array? result) "sort: array returns array")
    (assert-eq (get result 0) 1 "sort: array sorted first")
    (assert-eq (get result 1) 2 "sort: array sorted second")
    (assert-eq (get result 2) 3 "sort: array sorted third")
    (assert-true (identical? result arr) "sort: array mutated in place")))
(let ((result (sort [3 1 2])))
  (assert-true (array? result) "sort: array returns array")
  (assert-eq (get result 0) 1 "sort: array sorted first")
  (assert-eq (get result 1) 2 "sort: array sorted second")
  (assert-eq (get result 2) 3 "sort: array sorted third"))

## ── range ───────────────────────────────────────────────────────────
(let ((r (range 5)))
  (assert-true (array? r) "range: returns array")
  (assert-eq (length r) 5 "range: length")
  (assert-eq (get r 0) 0 "range: first")
  (assert-eq (get r 4) 4 "range: last"))
(let ((r (range 2 5)))
  (assert-eq (length r) 3 "range: start end length")
  (assert-eq (get r 0) 2 "range: start end first")
  (assert-eq (get r 2) 4 "range: start end last"))
(let ((r (range 0 10 3)))
  (assert-eq (length r) 4 "range: with step length")
  (assert-eq (get r 0) 0 "range: step first")
  (assert-eq (get r 1) 3 "range: step second")
  (assert-eq (get r 3) 9 "range: step last"))
(let ((r (range 5 0 -1)))
  (assert-eq (length r) 5 "range: negative step length")
  (assert-eq (get r 0) 5 "range: negative step first")
  (assert-eq (get r 4) 1 "range: negative step last"))
(let ((r (range 0)))
  (assert-true (array? r) "range: zero returns array")
  (assert-eq (length r) 0 "range: zero is empty"))
(let ((r (range 5 5)))
  (assert-eq (length r) 0 "range: start=end is empty"))

## ── apply ───────────────────────────────────────────────────────────
(assert-eq (apply + (list 1 2 3)) 6 "apply: spread list into +")
(assert-eq (apply + 10 (list 1 2 3)) 16 "apply: leading arg + spread")
(assert-eq (apply + 10 20 (list 1 2)) 33 "apply: two leading args + spread")
(assert-list-eq (apply list ()) () "apply: empty spread")
(assert-list-eq (apply list 1 (list 2 3)) (list 1 2 3) "apply: mixed args")
(assert-eq (apply + @[1 2 3]) 6 "apply: spread array")
(assert-eq (apply + [1 2 3]) 6 "apply: spread tuple")

## ── reduce ──────────────────────────────────────────────────────────
(assert-eq (reduce + 0 (list 1 2 3)) 6 "reduce: sum")
(assert-eq (reduce * 1 (list 2 3 4)) 24 "reduce: product")
(assert-eq (reduce + 0 ()) 0 "reduce: empty returns init")

## ── keep ────────────────────────────────────────────────────────────
(assert-list-eq (keep odd? (list 1 2 3 4 5)) (list 1 3 5) "keep: odd")
(assert-list-eq (keep even? (list 1 2 3 4 5)) (list 2 4) "keep: even")
(assert-list-eq (keep odd? ()) () "keep: empty")

## ── identity ────────────────────────────────────────────────────────
(assert-eq (identity 42) 42 "identity: number")
(assert-eq (identity nil) nil "identity: nil")
(assert-eq (identity true) true "identity: bool")
(assert-list-eq (identity (list 1 2)) (list 1 2) "identity: list")

## ── complement ──────────────────────────────────────────────────────
(assert-true ((complement nil?) 42) "complement: not nil")
(assert-false ((complement nil?) nil) "complement: nil")
(assert-true ((complement even?) 3) "complement: not even")
(assert-false ((complement even?) 4) "complement: even")

## ── constantly ──────────────────────────────────────────────────────
(assert-eq ((constantly 42) 1 2 3) 42 "constantly: ignores args")
(assert-eq ((constantly 42)) 42 "constantly: no args")
(assert-eq ((constantly nil) :a :b) nil "constantly: nil")

## ── compose / comp ──────────────────────────────────────────────────
(assert-eq ((compose) 42) 42 "compose: zero fns is identity")
(assert-eq ((compose (fn (x) (+ x 1))) 5) 6 "compose: single fn")
(assert-eq ((compose (fn (x) (* x 2)) (fn (x) (+ x 1))) 3) 8 "compose: f(g(x)) = (* 2 (+ 3 1))")
(assert-eq ((comp (fn (x) (+ x 1)) (fn (x) (* x 2))) 3) 7 "comp: alias, (+ 1 (* 2 3))")

## ── partial ─────────────────────────────────────────────────────────
(assert-eq ((partial + 10) 5) 15 "partial: one bound arg")
(assert-eq ((partial + 10 20) 5) 35 "partial: two bound args")
(assert-eq ((partial * 2 3) 4) 24 "partial: multiply")
(assert-list-eq ((partial list :a) :b :c) (list :a :b :c) "partial: variadic")

## ── juxt ────────────────────────────────────────────────────────────
(assert-list-eq ((juxt (fn (x) (* x 2)) (fn (x) (+ x 1))) 3)
                (list 6 4)
                "juxt: two fns")
(assert-list-eq ((juxt even? odd?) 4)
                (list true false)
                "juxt: predicates")

## ── all? ────────────────────────────────────────────────────────────
(assert-true (all? even? (list 2 4 6)) "all?: all even list")
(assert-false (all? even? (list 2 3 6)) "all?: not all even list")
(assert-true (all? even? ()) "all?: empty is vacuously true")
(assert-true (all? even? @[2 4 6]) "all?: array")
(assert-false (all? even? @[2 3 6]) "all?: array not all")
(assert-true (all? even? [2 4 6]) "all?: tuple")
(assert-false (all? even? [2 3 6]) "all?: tuple not all")

## ── any? ────────────────────────────────────────────────────────────
(assert-true (any? even? (list 1 2 3)) "any?: has even list")
(assert-false (any? even? (list 1 3 5)) "any?: no even list")
(assert-false (any? even? ()) "any?: empty is false")
(assert-true (any? even? @[1 2 3]) "any?: array")
(assert-false (any? even? @[1 3 5]) "any?: array none")
(assert-true (any? even? [1 2 3]) "any?: tuple")
(assert-false (any? even? [1 3 5]) "any?: tuple none")

## ── find ────────────────────────────────────────────────────────────
(assert-eq (find even? (list 1 3 4 5)) 4 "find: first even in list")
(assert-eq (find even? (list 1 3 5)) nil "find: not found returns nil")
(assert-eq (find even? ()) nil "find: empty returns nil")
(assert-eq (find even? @[1 3 4 5]) 4 "find: array")
(assert-eq (find even? [1 3 4 5]) 4 "find: tuple")

## ── find-index ──────────────────────────────────────────────────────
(assert-eq (find-index even? (list 1 3 4 5)) 2 "find-index: list")
(assert-eq (find-index even? (list 1 3 5)) nil "find-index: not found")
(assert-eq (find-index even? ()) nil "find-index: empty")
(assert-eq (find-index even? @[1 3 4 5]) 2 "find-index: array")
(assert-eq (find-index even? [1 3 4 5]) 2 "find-index: tuple")

## ── count ───────────────────────────────────────────────────────────
(assert-eq (count even? (list 1 2 3 4 5 6)) 3 "count: list")
(assert-eq (count even? ()) 0 "count: empty")
(assert-eq (count even? @[1 2 3 4 5 6]) 3 "count: array")
(assert-eq (count even? [1 2 3 4 5 6]) 3 "count: tuple")

## ── nth ─────────────────────────────────────────────────────────────
(assert-eq (nth 0 (list 10 20 30)) 10 "nth: first of list")
(assert-eq (nth 2 (list 10 20 30)) 30 "nth: last of list")
(assert-eq (nth 1 @[10 20 30]) 20 "nth: array")
(assert-eq (nth 1 [10 20 30]) 20 "nth: tuple")

## ── zip ─────────────────────────────────────────────────────────────
(let ((z (zip (list 1 2 3) (list :a :b :c))))
  (assert-eq (length z) 3 "zip: length")
  (assert-list-eq (first z) (list 1 :a) "zip: first pair")
  (assert-list-eq (first (rest z)) (list 2 :b) "zip: second pair"))
(let ((z (zip (list 1 2) (list :a :b :c))))
  (assert-eq (length z) 2 "zip: stops at shortest"))
(assert-list-eq (zip) () "zip: no args")
(let ((z (zip @[1 2 3] @[:a :b :c])))
  (assert-true (array? z) "zip: array input returns array")
  (assert-eq (length z) 3 "zip: array length"))
(let ((z (zip [1 2] [:a :b])))
  (assert-true (array? z) "zip: array input returns array"))

## ── flatten ─────────────────────────────────────────────────────────
(assert-list-eq (flatten (list 1 (list 2 3) (list 4 (list 5))))
                (list 1 2 3 4 5)
                "flatten: nested lists")
(assert-list-eq (flatten ()) () "flatten: empty")
(assert-list-eq (flatten (list 1 2 3)) (list 1 2 3) "flatten: already flat")
(let ((f (flatten @[1 @[2 3] @[4]])))
  (assert-true (array? f) "flatten: array returns array")
  (assert-eq (length f) 4 "flatten: array length"))

## ── take-while ──────────────────────────────────────────────────────
(assert-list-eq (take-while even? (list 2 4 5 6)) (list 2 4) "take-while: list")
(assert-list-eq (take-while even? (list 1 2 3)) () "take-while: none match")
(assert-list-eq (take-while even? ()) () "take-while: empty")
(let ((tw (take-while even? @[2 4 5 6])))
  (assert-true (array? tw) "take-while: array returns array")
  (assert-eq (length tw) 2 "take-while: array length"))
(let ((tw (take-while even? [2 4 5 6])))
  (assert-true (array? tw) "take-while: array returns array"))

## ── drop-while ──────────────────────────────────────────────────────
(assert-list-eq (drop-while even? (list 2 4 5 6)) (list 5 6) "drop-while: list")
(assert-list-eq (drop-while even? (list 1 2 3)) (list 1 2 3) "drop-while: none dropped")
(assert-list-eq (drop-while even? ()) () "drop-while: empty")
(let ((dw (drop-while even? @[2 4 5 6])))
  (assert-true (array? dw) "drop-while: array returns array")
  (assert-eq (length dw) 2 "drop-while: array length"))
(let ((dw (drop-while even? [2 4 5 6])))
  (assert-true (array? dw) "drop-while: array returns array"))

## ── distinct ────────────────────────────────────────────────────────
(assert-list-eq (distinct (list 1 2 1 3 2 4)) (list 1 2 3 4) "distinct: list")
(assert-list-eq (distinct ()) () "distinct: empty")
(let ((d (distinct @[1 2 1 3 2 4])))
  (assert-true (array? d) "distinct: array returns array")
  (assert-eq (length d) 4 "distinct: array deduped"))
(let ((d (distinct [1 2 1 3 2 4])))
  (assert-true (array? d) "distinct: array returns array"))

## ── frequencies ─────────────────────────────────────────────────────
(let ((freq (frequencies (list :a :b :a :c :b :a))))
  (assert-true (struct? freq) "frequencies: returns struct")
  (assert-eq (get freq :a) 3 "frequencies: a=3")
  (assert-eq (get freq :b) 2 "frequencies: b=2")
  (assert-eq (get freq :c) 1 "frequencies: c=1"))
(let ((freq (frequencies @[:a :b :a])))
  (assert-true (struct? freq) "frequencies: array input")
  (assert-eq (get freq :a) 2 "frequencies: array a=2"))
(let ((freq (frequencies ())))
  (assert-true (struct? freq) "frequencies: empty"))

## ── mapcat ──────────────────────────────────────────────────────────
(assert-list-eq (mapcat (fn (x) (list x (* x 10))) (list 1 2 3))
                (list 1 10 2 20 3 30)
                "mapcat: list")
(assert-list-eq (mapcat (fn (x) ()) (list 1 2 3))
                ()
                "mapcat: empty results")
(let ((mc (mapcat (fn (x) @[x (* x 10)]) @[1 2 3])))
  (assert-true (array? mc) "mapcat: array returns array")
  (assert-eq (length mc) 6 "mapcat: array length"))

## ── group-by ────────────────────────────────────────────────────────
(let ((groups (group-by even? (list 1 2 3 4 5 6))))
  (assert-true (struct? groups) "group-by: returns table")
  (assert-eq (length (get groups true)) 3 "group-by: evens count")
  (assert-eq (length (get groups false)) 3 "group-by: odds count"))
(let ((groups (group-by even? @[1 2 3 4 5 6])))
  (assert-true (struct? groups) "group-by: array input")
  (assert-eq (length (get groups true)) 3 "group-by: array evens"))

## ── map-indexed ─────────────────────────────────────────────────────
(assert-list-eq (map-indexed (fn (i x) (list i x)) (list :a :b :c))
                (list (list 0 :a) (list 1 :b) (list 2 :c))
                "map-indexed: list")
(assert-list-eq (map-indexed (fn (i x) (list i x)) ())
                ()
                "map-indexed: empty")
(let ((mi (map-indexed (fn (i x) (+ i x)) @[10 20 30])))
  (assert-true (array? mi) "map-indexed: array returns array")
  (assert-eq (get mi 0) 10 "map-indexed: 0+10")
  (assert-eq (get mi 1) 21 "map-indexed: 1+20")
  (assert-eq (get mi 2) 32 "map-indexed: 2+30"))

## ── partition ───────────────────────────────────────────────────────
(let ((p (partition 2 (list 1 2 3 4 5))))
  (assert-eq (length p) 3 "partition: list count")
  (assert-list-eq (first p) (list 1 2) "partition: first group")
  (assert-list-eq (last p) (list 5) "partition: last group partial"))
(let ((p (partition 2 @[1 2 3 4 5])))
  (assert-true (array? p) "partition: array returns array")
  (assert-eq (length p) 3 "partition: array count")
  (assert-true (array? (get p 0)) "partition: array chunks are arrays"))
(assert-list-eq (partition 3 ()) () "partition: empty")

## ── interpose ───────────────────────────────────────────────────────
(assert-list-eq (interpose :sep (list 1 2 3)) (list 1 :sep 2 :sep 3) "interpose: list")
(assert-list-eq (interpose :sep (list 1)) (list 1) "interpose: single")
(assert-list-eq (interpose :sep ()) () "interpose: empty")
(let ((ip (interpose :sep @[1 2 3])))
  (assert-true (array? ip) "interpose: array returns array")
  (assert-eq (length ip) 5 "interpose: array length"))

## ── min-key / max-key ───────────────────────────────────────────────
(assert-eq (min-key abs -3 1 -7 4) 1 "min-key: smallest abs")
(assert-eq (max-key abs -3 1 -7 4) -7 "max-key: largest abs")
(assert-eq (min-key identity 5 3 8 1) 1 "min-key: identity")
(assert-eq (max-key identity 5 3 8 1) 8 "max-key: identity")

## ── memoize ─────────────────────────────────────────────────────────
(let* ((call-count 0)
       (mf (memoize (fn (x)
                      (assign call-count (+ call-count 1))
                      (* x x)))))
  (assert-eq (mf 3) 9 "memoize: compute 3*3")
  (assert-eq (mf 3) 9 "memoize: cached 3*3")
  (assert-eq (mf 4) 16 "memoize: compute 4*4")
  (assert-eq call-count 2 "memoize: called twice not thrice"))

## ── sort-by ─────────────────────────────────────────────────────────
(assert-list-eq (sort-by abs (list -3 1 -2)) (list 1 -2 -3) "sort-by: abs list")
(assert-list-eq (sort-by identity (list 3 1 2)) (list 1 2 3) "sort-by: identity = sort")
(assert-list-eq (sort-by identity ()) () "sort-by: empty")
(let ((result (sort-by abs @[-3 1 -2])))
  (assert-true (array? result) "sort-by: array returns array")
  (assert-eq (get result 0) 1 "sort-by: array first")
  (assert-eq (get result 1) -2 "sort-by: array second")
  (assert-eq (get result 2) -3 "sort-by: array third"))
(let ((result (sort-by abs [3 1 2])))
  (assert-true (array? result) "sort-by: array returns array"))

## ── freeze / thaw: structs ───────────────────────────────────────────
(let ((t @{:a 1 :b 2}))
  (let ((s (freeze t)))
    (assert-true (struct? s) "freeze: returns struct")
    (assert-eq (get s :a) 1 "freeze: preserves values")
    (assert-eq (get s :b) 2 "freeze: preserves all keys")))

(let ((s {:a 1 :b 2}))
  (let ((t (thaw s)))
    (assert-true (struct? t) "thaw: returns @struct")
    (assert-eq (get t :a) 1 "thaw: preserves values")
    (put t :c 3)
    (assert-eq (get t :c) 3 "thaw: result is mutable")))

## ── freeze / thaw: arrays ───────────────────────────────────────────
(let ((ma @[1 2 3]))
  (let ((a (freeze ma)))
    (assert-true (array? a) "freeze @array: returns array")
    (assert-eq (get a 0) 1 "freeze @array: preserves values")
    (assert-eq (length a) 3 "freeze @array: preserves length")))

(let ((a [1 2 3]))
  (let ((ma (thaw a)))
    (assert-true (@array? ma) "thaw array: returns @array")
    (assert-eq (get ma 0) 1 "thaw array: preserves values")
    (push ma 4)
    (assert-eq (length ma) 4 "thaw array: result is mutable")))

(assert-eq (freeze [1 2 3]) [1 2 3] "freeze: immutable array returns as-is")
(assert-eq (type-of (thaw (thaw [1 2 3]))) :@array "thaw: idempotent on @array")

## ── freeze / thaw: strings ──────────────────────────────────────────
(assert-eq (freeze @"hello") "hello" "freeze @string: returns string")
(assert-eq (type-of (freeze @"hello")) :string "freeze @string: type is string")
(assert-eq (freeze "hello") "hello" "freeze: immutable string returns as-is")

(assert-eq (type-of (thaw "hello")) :@string "thaw string: returns @string")
(assert-eq (freeze (thaw "hello")) "hello" "thaw then freeze: roundtrip")
(assert-eq (type-of (thaw (thaw "hello"))) :@string "thaw: idempotent on @string")

## ── freeze / thaw: bytes ────────────────────────────────────────────
(let ((mb (@bytes 1 2 3)))
  (let ((b (freeze mb)))
    (assert-true (bytes? b) "freeze @bytes: returns bytes")
    (assert-eq (get b 0) 1 "freeze @bytes: preserves values")))

(let ((b (bytes 1 2 3)))
  (let ((mb (thaw b)))
    (assert-true (@bytes? mb) "thaw bytes: returns @bytes")
    (assert-eq (get mb 0) 1 "thaw bytes: preserves values")))

(assert-eq (type-of (freeze (freeze (@bytes 1 2 3)))) :bytes "freeze: idempotent on bytes")
(assert-eq (type-of (thaw (thaw (bytes 1 2 3)))) :@bytes "thaw: idempotent on @bytes")

## ── freeze @string: invalid UTF-8 ──────────────────────────────────
(assert-err (fn [] (freeze (@string 255 254))) "freeze @string with invalid UTF-8 signals error")
