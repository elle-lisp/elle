(import-file "./examples/assertions.lisp")

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
  (assert-true (tuple? result) "sort: tuple returns tuple")
  (assert-eq (get result 0) 1 "sort: tuple sorted first")
  (assert-eq (get result 1) 2 "sort: tuple sorted second")
  (assert-eq (get result 2) 3 "sort: tuple sorted third"))

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
