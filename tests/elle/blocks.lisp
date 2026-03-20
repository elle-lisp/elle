(elle/epoch 1)
# Tests for named blocks with break
#
# Note: block expressions containing break must be bound to a var before
# passing to assert-eq, due to the closure-return bug (same as match).


# ============================================================================
# Anonymous blocks
# ============================================================================

(assert (= (block 1 2 3) 3) "block returns last")
(assert (= (block) nil) "block empty returns nil")
(assert (= (block 42) 42) "block single value")

# ============================================================================
# Named blocks
# ============================================================================

(assert (= (block :done 1 2 3) 3) "named block returns last")
(assert (= (block :done) nil) "named block empty body")

# ============================================================================
# Break from anonymous block
# ============================================================================

(let ([result (block (break 42) 99)])
  (assert (= result 42) "break anonymous with value"))

(let ([result (block (break) 99)])
  (assert (= result nil) "break anonymous nil"))

# ============================================================================
# Break from named block
# ============================================================================

(let ([result (block :done (break :done 42) 99)])
  (assert (= result 42) "break named with value"))

(let ([result (block :done (break :done) 99)])
  (assert (= result nil) "break named nil"))

# ============================================================================
# Nested blocks
# ============================================================================

(let ([result (block :outer (block :inner (break :outer 42) 1) 2)])
  (assert (= result 42) "break outer from inner"))

(let ([result (block :outer (block :inner (break :inner 10) 1) 2)])
  (assert (= result 2) "break inner continues outer"))

(let ([result (+ 1 (block :inner (break :inner 10) 99))])
  (assert (= result 11) "break inner value used by outer"))

# ============================================================================
# Break in control flow
# ============================================================================

(let ([result (block :done (if true (break :done 42) 0) 99)])
  (assert (= result 42) "break in if true"))

(let ([result (block :done (if false (break :done 42) 0) 99)])
  (assert (= result 99) "break in if false"))

(begin
  (var i 0)
  (let ([result
    (block :done
      (while true
        (begin
          (if (= i 5) (break :done i) nil)
          (assign i (+ i 1)))))])
    (assert (= result 5) "break in loop")))

# ============================================================================
# Scope isolation
# ============================================================================

(assert (= ((fn ()
     (var x 1)
     (block (var x 2) x)
     x)) 1) "block creates scope")

# ============================================================================
# Multiple breaks
# ============================================================================

(let ([result (block :done (break :done 1) (break :done 2) 3)])
  (assert (= result 1) "first break wins"))

(let ([result (block :done (if true (break :done 10) (break :done 20)) 99)])
  (assert (= result 10) "conditional breaks true"))

(let ([result (block :done (if false (break :done 10) (break :done 20)) 99)])
  (assert (= result 20) "conditional breaks false"))

# ============================================================================
# Break with expressions
# ============================================================================

(let ([result (block :done (break :done (+ 20 22)) 99)])
  (assert (= result 42) "break with computed value"))

(let ([result (block :done (let ((x 42)) (break :done x)) 99)])
  (assert (= result 42) "break with let value"))

# ============================================================================
# Break in while loops
# ============================================================================

(begin
  (var i 0)
  (let ([result
    (while true
      (begin
        (if (= i 5) (break :while i) nil)
        (assign i (+ i 1))))])
    (assert (= result 5) "break in while")))

(begin
  (var i 0)
  (while true
    (begin
      (if (= i 3) (break nil) nil)
      (assign i (+ i 1))))
  (assert (= i 3) "break in while unnamed"))

(begin
  (var i 0)
  (let ([result (while (< i 3) (assign i (+ i 1)))])
    (assert (= result nil) "while without break")))

# ============================================================================
# Break with value in while
# ============================================================================

(begin
  (var i 0)
  (let ([result
    (while true
      (begin
        (assign i (+ i 1))
        (if (= i 3) (break 42) nil)))])
    (assert (= result 42) "break in while with value")))

(begin
  (var total 0)
  (var outer 0)
  (while (< outer 3)
    (begin
      (var inner 0)
      (while true
        (begin
          (if (= inner 2) (break) nil)
          (assign total (+ total 1))
          (assign inner (+ inner 1))))
      (assign outer (+ outer 1))))
  (assert (= total 6) "break in nested while inner"))

# ============================================================================
# Compile-time error tests (from integration/blocks.rs)
# ============================================================================

# break_outside_block_error
(let (([ok? _] (protect ((fn () (eval '(break 42))))))) (assert (not ok?) "break outside block is compile error"))

# break_unknown_name_error
(let (([ok? _] (protect ((fn () (eval '(block :a (break :b 42)))))))) (assert (not ok?) "break with unknown block name is compile error"))

# break_across_fn_boundary_error
(let (([ok? _] (protect ((fn () (eval '(block :done ((fn () (break :done 42)))))))))) (assert (not ok?) "break across function boundary is compile error"))

(begin
  (var sum 0)
  (var i 0)
  (while (< i 3)
    (begin
      (let ([inner-result (while true (break 10))])
        (assign sum (+ sum inner-result)))
      (assign i (+ i 1))))
  (assert (= sum 30) "break in nested while with value"))

# ============================================================================
# Break in each
# ============================================================================

(begin
  (var last nil)
  (each x '(1 2 3 4 5)
    (begin
      (assign last x)
      (if (= x 3) (break) nil)))
  (assert (= last 3) "break in each list"))

(let ([result
  (each x '(10 20 30 40)
    (if (= x 30) (break :while :found) nil))])
  (assert (= result :found) "break in each with value"))

(let ([result (each x '(1 2 3) x)])
  (assert (= result nil) "each without break"))

(let ([result
  (each x @[100 200 300 400]
    (if (= x 300) (break x) nil))])
  (assert (= result 300) "break in each array"))

(begin
  (var count 0)
  (let ([result
    (each ch "hello"
      (begin
        (assign count (+ count 1))
        (if (= ch "l") (break count) nil)))])
    (assert (= result 3) "break in each string")))

# ============================================================================
# Compile-time error tests (from integration/blocks.rs)
# ============================================================================

# break_outside_block_error
(let (([ok? _] (protect ((fn () (eval '(break 42))))))) (assert (not ok?) "break outside block is compile error"))

# break_unknown_name_error
(let (([ok? _] (protect ((fn () (eval '(block :a (break :b 42)))))))) (assert (not ok?) "break with unknown block name is compile error"))

# break_across_fn_boundary_error
(let (([ok? _] (protect ((fn () (eval '(block :done ((fn () (break :done 42)))))))))) (assert (not ok?) "break across function boundary is compile error"))
