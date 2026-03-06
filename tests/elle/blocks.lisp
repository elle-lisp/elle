# Tests for named blocks with break
#
# Note: block expressions containing break must be bound to a var before
# passing to assert-eq, due to the closure-return bug (same as match).

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Anonymous blocks
# ============================================================================

(assert-eq (block 1 2 3) 3 "block returns last")
(assert-eq (block) nil "block empty returns nil")
(assert-eq (block 42) 42 "block single value")

# ============================================================================
# Named blocks
# ============================================================================

(assert-eq (block :done 1 2 3) 3 "named block returns last")
(assert-eq (block :done) nil "named block empty body")

# ============================================================================
# Break from anonymous block
# ============================================================================

(let ([result (block (break 42) 99)])
  (assert-eq result 42 "break anonymous with value"))

(let ([result (block (break) 99)])
  (assert-eq result nil "break anonymous nil"))

# ============================================================================
# Break from named block
# ============================================================================

(let ([result (block :done (break :done 42) 99)])
  (assert-eq result 42 "break named with value"))

(let ([result (block :done (break :done) 99)])
  (assert-eq result nil "break named nil"))

# ============================================================================
# Nested blocks
# ============================================================================

(let ([result (block :outer (block :inner (break :outer 42) 1) 2)])
  (assert-eq result 42 "break outer from inner"))

(let ([result (block :outer (block :inner (break :inner 10) 1) 2)])
  (assert-eq result 2 "break inner continues outer"))

(let ([result (+ 1 (block :inner (break :inner 10) 99))])
  (assert-eq result 11 "break inner value used by outer"))

# ============================================================================
# Break in control flow
# ============================================================================

(let ([result (block :done (if true (break :done 42) 0) 99)])
  (assert-eq result 42 "break in if true"))

(let ([result (block :done (if false (break :done 42) 0) 99)])
  (assert-eq result 99 "break in if false"))

(begin
  (var i 0)
  (let ([result
    (block :done
      (while true
        (begin
          (if (= i 5) (break :done i) nil)
          (set i (+ i 1)))))])
    (assert-eq result 5 "break in loop")))

# ============================================================================
# Scope isolation
# ============================================================================

(assert-eq
  ((fn ()
     (var x 1)
     (block (var x 2) x)
     x))
  1
  "block creates scope")

# ============================================================================
# Multiple breaks
# ============================================================================

(let ([result (block :done (break :done 1) (break :done 2) 3)])
  (assert-eq result 1 "first break wins"))

(let ([result (block :done (if true (break :done 10) (break :done 20)) 99)])
  (assert-eq result 10 "conditional breaks true"))

(let ([result (block :done (if false (break :done 10) (break :done 20)) 99)])
  (assert-eq result 20 "conditional breaks false"))

# ============================================================================
# Break with expressions
# ============================================================================

(let ([result (block :done (break :done (+ 20 22)) 99)])
  (assert-eq result 42 "break with computed value"))

(let ([result (block :done (let ((x 42)) (break :done x)) 99)])
  (assert-eq result 42 "break with let value"))

# ============================================================================
# Break in while loops
# ============================================================================

(begin
  (var i 0)
  (let ([result
    (while true
      (begin
        (if (= i 5) (break :while i) nil)
        (set i (+ i 1))))])
    (assert-eq result 5 "break in while")))

(begin
  (var i 0)
  (while true
    (begin
      (if (= i 3) (break nil) nil)
      (set i (+ i 1))))
  (assert-eq i 3 "break in while unnamed"))

(begin
  (var i 0)
  (let ([result (while (< i 3) (set i (+ i 1)))])
    (assert-eq result nil "while without break")))

# ============================================================================
# Break with value in while
# ============================================================================

(begin
  (var i 0)
  (let ([result
    (while true
      (begin
        (set i (+ i 1))
        (if (= i 3) (break 42) nil)))])
    (assert-eq result 42 "break in while with value")))

(begin
  (var total 0)
  (var outer 0)
  (while (< outer 3)
    (begin
      (var inner 0)
      (while true
        (begin
          (if (= inner 2) (break) nil)
          (set total (+ total 1))
          (set inner (+ inner 1))))
      (set outer (+ outer 1))))
  (assert-eq total 6 "break in nested while inner"))

(begin
  (var sum 0)
  (var i 0)
  (while (< i 3)
    (begin
      (let ([inner-result (while true (break 10))])
        (set sum (+ sum inner-result)))
      (set i (+ i 1))))
  (assert-eq sum 30 "break in nested while with value"))

# ============================================================================
# Break in each
# ============================================================================

(begin
  (var last nil)
  (each x '(1 2 3 4 5)
    (begin
      (set last x)
      (if (= x 3) (break) nil)))
  (assert-eq last 3 "break in each list"))

(let ([result
  (each x '(10 20 30 40)
    (if (= x 30) (break :while :found) nil))])
  (assert-eq result :found "break in each with value"))

(let ([result (each x '(1 2 3) x)])
  (assert-eq result nil "each without break"))

(let ([result
  (each x @[100 200 300 400]
    (if (= x 300) (break x) nil))])
  (assert-eq result 300 "break in each array"))

(begin
  (var count 0)
  (let ([result
    (each ch "hello"
      (begin
        (set count (+ count 1))
        (if (= ch "l") (break count) nil)))])
    (assert-eq result 3 "break in each string")))
