# Macro desugaring tests
#
# Migrated from tests/property/macros.rs
# These tests verify that macro desugaring produces correct results.
# The desugaring is structural (not data-dependent), so representative
# hardcoded examples suffice instead of property-based generation.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# defn equivalence tests
# ============================================================================

# defn produces same result as def + fn
(begin
  (defn f1 (x y) (+ x y))
  (assert-eq (f1 3 5) 8 "defn basic two params"))

(begin
  (def f2 (fn (x y) (+ x y)))
  (assert-eq (f2 3 5) 8 "def+fn basic two params"))

(begin
  (defn f3 (x y) (+ x y))
  (def f4 (fn (x y) (+ x y)))
  (assert-eq (f3 -10 7) (f4 -10 7) "defn and def+fn produce same result"))

# defn with multiple body expressions returns last
(begin
  (defn f5 (x y) (+ x 1) (+ x y))
  (assert-eq (f5 10 20) 30 "defn multiple body expressions"))

(begin
  (defn f6 (x y) (- x 5) (* x 2) (+ x y))
  (assert-eq (f6 5 3) 8 "defn three body expressions"))

# defn with single parameter
(begin
  (defn double (x) (* x 2))
  (assert-eq (double 5) 10 "defn single param positive"))

(begin
  (defn double2 (x) (* x 2))
  (assert-eq (double2 -3) -6 "defn single param negative"))

(begin
  (defn double3 (x) (* x 2))
  (assert-eq (double3 0) 0 "defn single param zero"))

# defn with three parameters
(begin
  (defn sum3 (a b c) (+ a (+ b c)))
  (assert-eq (sum3 1 2 3) 6 "defn three params positive"))

(begin
  (defn sum3b (a b c) (+ a (+ b c)))
  (assert-eq (sum3b -5 10 -3) 2 "defn three params mixed"))

(begin
  (defn sum3c (a b c) (+ a (+ b c)))
  (assert-eq (sum3c 0 0 0) 0 "defn three params zero"))

# defn with conditional body (absolute value)
(begin
  (defn abs1 (x) (if (< x 0) (- 0 x) x))
  (assert-eq (abs1 -5) 5 "defn conditional negative"))

(begin
  (defn abs2 (x) (if (< x 0) (- 0 x) x))
  (assert-eq (abs2 10) 10 "defn conditional positive"))

(begin
  (defn abs3 (x) (if (< x 0) (- 0 x) x))
  (assert-eq (abs3 0) 0 "defn conditional zero"))

# defn recursive (factorial)
(begin
  (defn fact1 (n) (if (= n 0) 1 (* n (fact1 (- n 1)))))
  (assert-eq (fact1 0) 1 "defn recursive factorial 0"))

(begin
  (defn fact2 (n) (if (= n 0) 1 (* n (fact2 (- n 1)))))
  (assert-eq (fact2 5) 120 "defn recursive factorial 5"))

(begin
  (defn fact3 (n) (if (= n 0) 1 (* n (fact3 (- n 1)))))
  (assert-eq (fact3 3) 6 "defn recursive factorial 3"))

# ============================================================================
# let* sequential binding tests
# ============================================================================

# let* allows later bindings to reference earlier ones
(begin
  (let* ((x 5) (y (+ x 3)))
    (assert-eq y 8 "let* sequential binding positive")))

(begin
  (let* ((x -10) (y (+ x 5)))
    (assert-eq y -5 "let* sequential binding negative")))

(begin
  (let* ((x 0) (y (+ x 0)))
    (assert-eq y 0 "let* sequential binding zero")))

# let* is equivalent to nested let
(begin
  (let* ((x 5) (y 3))
    (assert-eq (+ x y) 8 "let* two bindings")))

(begin
  (let ((x 5))
    (let ((y 3))
      (assert-eq (+ x y) 8 "nested let two bindings"))))

(begin
  (let* ((x 5) (y 3))
    (let ((x 5))
      (let ((y 3))
        (assert-eq (+ x y) 8 "let* and nested let equivalent")))))

# let* with three sequential bindings
(begin
  (let* ((x 1) (y (+ x 2)) (z (+ y 3)))
    (assert-eq z 6 "let* three sequential bindings")))

(begin
  (let* ((x -5) (y (+ x 10)) (z (+ y -2)))
    (assert-eq z 3 "let* three sequential mixed")))

(begin
  (let* ((x 0) (y (+ x 0)) (z (+ y 0)))
    (assert-eq z 0 "let* three sequential zero")))

# let* with empty bindings returns body
(begin
  (let* () 42)
  (assert-eq (let* () 42) 42 "let* empty bindings"))

(begin
  (assert-eq (let* () -10) -10 "let* empty bindings negative"))

(begin
  (assert-eq (let* () 0) 0 "let* empty bindings zero"))

# let* with single binding
(begin
  (let* ((y 7))
    (assert-eq y 7 "let* single binding")))

(begin
  (let* ((y -3))
    (assert-eq y -3 "let* single binding negative")))

(begin
  (let* ((y 0))
    (assert-eq y 0 "let* single binding zero")))

# let* with computed bindings
(begin
  (let* ((y (* 5 2)) (z (+ y 1)))
    (assert-eq z 11 "let* computed bindings positive")))

(begin
  (let* ((y (* -3 2)) (z (+ y 1)))
    (assert-eq z -5 "let* computed bindings negative")))

(begin
  (let* ((y (* 0 2)) (z (+ y 1)))
    (assert-eq z 1 "let* computed bindings zero")))

# ============================================================================
# Thread-first (->) tests
# ============================================================================

# (-> v (+ a)) is equivalent to (+ v a)
(begin
  (assert-eq (-> 5 (+ 3)) 8 "thread-first single positive"))

(begin
  (assert-eq (-> -10 (+ 5)) -5 "thread-first single negative"))

(begin
  (assert-eq (-> 0 (+ 0)) 0 "thread-first single zero"))

# (-> v (+ a) (* b)) is equivalent to (* (+ v a) b)
(begin
  (assert-eq (-> 2 (+ 3) (* 4)) 20 "thread-first chain positive"))

(begin
  (assert-eq (-> -5 (+ 10) (* 2)) 10 "thread-first chain mixed"))

(begin
  (assert-eq (-> 0 (+ 0) (* 5)) 0 "thread-first chain zero"))

# thread-first with three operations
(begin
  (assert-eq (-> 1 (+ 2) (* 3) (- 1)) 8 "thread-first three ops positive"))

(begin
  (assert-eq (-> -2 (+ 5) (* 2) (- 3)) 3 "thread-first three ops mixed"))

(begin
  (assert-eq (-> 0 (+ 1) (* 2) (- 0)) 2 "thread-first three ops zero"))

# thread-first identity: (-> v) == v
(begin
  (assert-eq (-> 42) 42 "thread-first identity positive"))

(begin
  (assert-eq (-> -7) -7 "thread-first identity negative"))

(begin
  (assert-eq (-> 0) 0 "thread-first identity zero"))

# ============================================================================
# Thread-last (->>) tests
# ============================================================================

# (->> v (- a)) is equivalent to (- a v)
(begin
  (assert-eq (->> 3 (- 10)) 7 "thread-last single positive"))

(begin
  (assert-eq (->> 5 (- -10)) -15 "thread-last single negative"))

(begin
  (assert-eq (->> 0 (- 0)) 0 "thread-last single zero"))

# (->> v (- a) (- b)) is equivalent to (- b (- a v))
(begin
  (assert-eq (->> 2 (- 10) (- 5)) -3 "thread-last chain positive"))

(begin
  (assert-eq (->> -3 (- 5) (- -2)) -10 "thread-last chain mixed"))

(begin
  (assert-eq (->> 0 (- 0) (- 0)) 0 "thread-last chain zero"))

# thread-last identity: (->> v) == v
(begin
  (assert-eq (->> 42) 42 "thread-last identity positive"))

(begin
  (assert-eq (->> -7) -7 "thread-last identity negative"))

(begin
  (assert-eq (->> 0) 0 "thread-last identity zero"))

# ============================================================================
# Block and break tests
# ============================================================================

# block returns last expression
(begin
  (assert-eq (block 1 2 3) 3 "block returns last positive"))

(begin
  (assert-eq (block -5 -10 -3) -3 "block returns last negative"))

(begin
  (assert-eq (block 0 0 0) 0 "block returns last zero"))

# break short-circuits
(begin
  (let ([result (block (break 42) 99)])
    (assert-eq result 42 "break short-circuits positive")))

(begin
  (let ([result (block (break -7) 99)])
    (assert-eq result -7 "break short-circuits negative")))

(begin
  (let ([result (block (break 0) 99)])
    (assert-eq result 0 "break short-circuits zero")))

# named break targets correct block
(begin
  (let ([result (block :outer (block :inner (break :outer 42) 1) 999)])
    (assert-eq result 42 "named break targets outer positive")))

(begin
  (let ([result (block :outer (block :inner (break :outer -5) 1) 999)])
    (assert-eq result -5 "named break targets outer negative")))

(begin
  (let ([result (block :outer (block :inner (break :outer 0) 1) 999)])
    (assert-eq result 0 "named break targets outer zero")))

# nested break targets inner
(begin
  (let ([result (block :outer (block :inner (break :inner 10) 1) 2)])
    (assert-eq result 2 "nested break targets inner positive")))

(begin
  (let ([result (block :outer (block :inner (break :inner -5) 1) 3)])
    (assert-eq result 3 "nested break targets inner negative")))

(begin
  (let ([result (block :outer (block :inner (break :inner 0) 1) 0)])
    (assert-eq result 0 "nested break targets inner zero")))

# block with multiple expressions
(begin
  (assert-eq (block 1 2 3) 3 "block multiple exprs positive"))

(begin
  (assert-eq (block -10 -5 -1) -1 "block multiple exprs negative"))

(begin
  (assert-eq (block 0 0 0) 0 "block multiple exprs zero"))

# block scope isolation
(begin
  (let ((x 1))
    (block (let ((x 2)) x))
    (assert-eq x 1 "block scope isolation positive")))

(begin
  (let ((x -5))
    (block (let ((x 10)) x))
    (assert-eq x -5 "block scope isolation negative")))

(begin
  (let ((x 0))
    (block (let ((x 0)) x))
    (assert-eq x 0 "block scope isolation zero")))

# ============================================================================
# Macro hygiene tests
# ============================================================================

# when returns body when true
(begin
  (assert-eq (when true 42) 42 "when true positive"))

(begin
  (assert-eq (when true -7) -7 "when true negative"))

(begin
  (assert-eq (when true 0) 0 "when true zero"))

# unless returns body when false
(begin
  (assert-eq (unless false 42) 42 "unless false positive"))

(begin
  (assert-eq (unless false -7) -7 "unless false negative"))

(begin
  (assert-eq (unless false 0) 0 "unless false zero"))

# nested defn visible to siblings
(begin
  (defn outer1 (x)
    (defn inner1 (y) (+ y x))
    (inner1 5))
  (assert-eq (outer1 10) 15 "nested defn visible positive"))

(begin
  (defn outer2 (x)
    (defn inner2 (y) (+ y x))
    (inner2 -3))
  (assert-eq (outer2 7) 4 "nested defn visible mixed"))

(begin
  (defn outer3 (x)
    (defn inner3 (y) (+ y x))
    (inner3 0))
  (assert-eq (outer3 0) 0 "nested defn visible zero"))

# let* inside defn
(begin
  (defn f_let1 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      z))
  (assert-eq (f_let1 0) 8 "let* inside defn positive"))

(begin
  (defn f_let2 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      z))
  (assert-eq (f_let2 -5) 3 "let* inside defn negative"))

(begin
  (defn f_let3 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      z))
  (assert-eq (f_let3 -8) 0 "let* inside defn zero"))

# thread-first inside defn
(begin
  (defn f_thread1 (x) (-> x (+ 5)))
  (assert-eq (f_thread1 10) 15 "thread-first inside defn positive"))

(begin
  (defn f_thread2 (x) (-> x (+ 5)))
  (assert-eq (f_thread2 -10) -5 "thread-first inside defn negative"))

(begin
  (defn f_thread3 (x) (-> x (+ 5)))
  (assert-eq (f_thread3 -5) 0 "thread-first inside defn zero"))

# thread-last inside defn
(begin
  (defn f_thread_last1 (x) (->> x (- 10)))
  (assert-eq (f_thread_last1 3) 7 "thread-last inside defn positive"))

(begin
  (defn f_thread_last2 (x) (->> x (- 10)))
  (assert-eq (f_thread_last2 -5) 15 "thread-last inside defn negative"))

(begin
  (defn f_thread_last3 (x) (->> x (- 10)))
  (assert-eq (f_thread_last3 0) 10 "thread-last inside defn zero"))

# block inside defn
(begin
  (defn f_block1 (x) (block (break 42) 99))
  (assert-eq (f_block1 0) 42 "block inside defn positive"))

(begin
  (defn f_block2 (x) (block (break -7) 99))
  (assert-eq (f_block2 0) -7 "block inside defn negative"))

(begin
  (defn f_block3 (x) (block (break 0) 99))
  (assert-eq (f_block3 0) 0 "block inside defn zero"))

# ============================================================================
# Combined/integration tests
# ============================================================================

# defn with let* and thread-first
(begin
  (defn f_combined1 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      (-> z (* 2))))
  (assert-eq (f_combined1 0) 16 "defn+let*+thread-first positive"))

(begin
  (defn f_combined2 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      (-> z (* 2))))
  (assert-eq (f_combined2 -5) 6 "defn+let*+thread-first negative"))

(begin
  (defn f_combined3 (x)
    (let* ((y (+ x 5)) (z (+ y 3)))
      (-> z (* 2))))
  (assert-eq (f_combined3 -8) 0 "defn+let*+thread-first zero"))

# nested blocks with named breaks
(begin
  (let ([result (block :a (block :b (block :c (break :b 10) 1) 2) 3)])
    (assert-eq result 3 "nested blocks named breaks positive")))

(begin
  (let ([result (block :a (block :b (block :c (break :b -5) 1) 2) 3)])
    (assert-eq result 3 "nested blocks named breaks negative")))

(begin
  (let ([result (block :a (block :b (block :c (break :b 0) 1) 2) 3)])
    (assert-eq result 3 "nested blocks named breaks zero")))

# defn with block and break
(begin
  (defn f_block_break1 (x)
    (block (if (< x 0) (break 99) (+ x 1))))
  (assert-eq (f_block_break1 5) 6 "defn+block+break positive"))

(begin
  (defn f_block_break2 (x)
    (block (if (< x 0) (break 99) (+ x 1))))
  (assert-eq (f_block_break2 -5) 99 "defn+block+break negative"))

(begin
  (defn f_block_break3 (x)
    (block (if (< x 0) (break 99) (+ x 1))))
  (assert-eq (f_block_break3 0) 1 "defn+block+break zero"))

# let* with thread-first
(begin
  (let* ((x 5) (y (-> x (+ 3))))
    (assert-eq y 8 "let*+thread-first positive")))

(begin
  (let* ((x -10) (y (-> x (+ 5))))
    (assert-eq y -5 "let*+thread-first negative")))

(begin
  (let* ((x 0) (y (-> x (+ 0))))
    (assert-eq y 0 "let*+thread-first zero")))

# let* with thread-last
(begin
  (let* ((x 5) (y (->> x (- 10))))
    (assert-eq y 5 "let*+thread-last positive")))

(begin
  (let* ((x -5) (y (->> x (- 10))))
    (assert-eq y 15 "let*+thread-last negative")))

(begin
  (let* ((x 0) (y (->> x (- 0))))
    (assert-eq y 0 "let*+thread-last zero")))

# ============================================================================
# Macro hygiene tests (migrated from tests/integration/hygiene.rs)
# ============================================================================

# SECTION 1: Macro hygiene — no accidental capture

# The swap macro introduces a `tmp` binding. The caller also has `tmp`.
# The macro's `tmp` must not shadow the caller's `tmp`.
(begin
  (defmacro my-swap (a b)
    `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

  (let ((tmp 10) (x 1) (y 2))
    (my-swap x y)
    (assert-eq tmp 10 "test_macro_no_capture")))

# The macro introduces an internal binding. The caller should not
# be able to see it.
(begin
  (defmacro with-internal (body)
    `(let ((internal-var 42)) ,body))

  (assert-eq (with-internal (+ 1 2)) 3 "test_macro_no_leak"))

# Two different macros both introduce `tmp`. They must not interfere.
(begin
  (defmacro add-tmp-a (x)
    `(let ((tmp ,x)) (+ tmp 1)))

  (defmacro add-tmp-b (x)
    `(let ((tmp ,x)) (+ tmp 2)))

  (assert-eq (+ (add-tmp-a 10) (add-tmp-b 20)) 33 "test_nested_macro_hygiene"))

# SECTION 2: Non-macro code unchanged

# Code without macros should work identically.
(begin
  (let ((x 10) (y 20))
    (assert-eq (+ x y) 30 "test_non_macro_code_unchanged")))

# Normal shadowing (no macros) should still work.
(begin
  (let ((x 10))
    (let ((x 20))
      (assert-eq x 20 "test_non_macro_shadowing_unchanged"))))

# SECTION 3: Macro argument resolution

# Macro argument variable reference resolves to the caller's binding.
(begin
  (defmacro double (x)
    `(+ ,x ,x))

  (let ((val 7))
    (assert-eq (double val) 14 "test_macro_with_expression_arg")))

# A macro-generated closure should capture a call-site variable correctly.
(begin
  (defmacro make-adder (n)
    `(fn (x) (+ x ,n)))

  (let ((amount 5))
    (let ((f (make-adder amount)))
      (assert-eq (f 10) 15 "test_macro_closure_captures_callsite"))))

# SECTION 4: Macro with conditional body (regression)

# This was a regression: wrapping false in a syntax object made it truthy.
# The hybrid wrapping approach (atoms via Quote, compounds via SyntaxLiteral)
# fixes this.
(begin
  (defmacro when-true (cond body)
    `(if ,cond ,body nil))

  (assert-eq (when-true false 42) nil "test_macro_with_conditional_body_regression"))

(begin
  (defmacro when-true2 (cond body)
    `(if ,cond ,body nil))

  (assert-eq (when-true2 true 42) 42 "test_macro_with_conditional_body_true"))

# SECTION 5: Swap macro end-to-end

# Verify the swap macro actually swaps values, not just that it's hygienic.
(begin
  (defmacro my-swap2 (a b)
    `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

  (let ((x 1) (y 2))
    (my-swap2 x y)
    (assert-eq (list x y) (list 2 1) "test_swap_actually_swaps")))

# The real hygiene test: swap when caller has a variable named `tmp`.
(begin
  (defmacro my-swap3 (a b)
    `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

  (let ((tmp 100) (x 1) (y 2))
    (my-swap3 x y)
    (assert-eq (list tmp x y) (list 100 2 1) "test_swap_with_same_named_tmp")))

# SECTION 6: gensym returns symbols (not strings)

# gensym should return a symbol that works in quasiquote templates.
# This was broken (#306): gensym returned a string, producing
# string literals where symbols were needed.
(begin
  (defmacro with-temp (body)
    (let ((tmp (gensym "tmp")))
      `(let ((,tmp 42)) ,body)))

  (assert-eq (with-temp (+ 1 2)) 3 "test_gensym_in_macro"))

# Macro A expands to code that invokes macro B, passing A's arguments
# through to B. Arguments from A's call site must retain their scopes
# through B's expansion. This exercises the Value::syntax round-trip
# for nested expansions.
(begin
  (defmacro inner-add (x y)
    `(+ ,x ,y))

  (defmacro outer-add (a b)
    `(inner-add ,a ,b))

  (let ((x 10) (y 20))
    (assert-eq (outer-add x y) 30 "test_nested_macro_scope_preservation")))

# Two gensym calls produce different symbols, so two macro
# expansions don't interfere.
(begin
  (defmacro bind-val (val body)
    (let ((g (gensym "v")))
      `(let ((,g ,val)) ,body)))

  (assert-eq (bind-val 10 (bind-val 20 (+ 1 2))) 3 "test_gensym_produces_unique_bindings"))

# SECTION 7: datum->syntax — hygiene escape hatch

# datum->syntax creates an `it` binding visible at the call site.
# This is the canonical anaphoric macro use case.
(begin
  (defmacro aif (test then else)
    `(let ((,(datum->syntax test 'it) ,test))
       (if ,(datum->syntax test 'it) ,then ,else)))

  (assert-eq (aif 42 it 0) 42 "test_anaphoric_if"))

# When the test is falsy, the else branch is taken.
(begin
  (defmacro aif2 (test then else)
    `(let ((,(datum->syntax test 'it) ,test))
       (if ,(datum->syntax test 'it) ,then ,else)))

  (assert-eq (aif2 false 42 0) 0 "test_anaphoric_if_false_branch"))

# datum->syntax works when the test is a compound expression.
(begin
  (defmacro aif3 (test then else)
    `(let ((,(datum->syntax test 'it) ,test))
       (if ,(datum->syntax test 'it) ,then ,else)))

  (assert-eq (aif3 (+ 1 2) (+ it 10) 0) 13 "test_anaphoric_if_with_expression"))

# An outer `it` binding should not be affected by the macro's `it`.
(begin
  (defmacro aif4 (test then else)
    `(let ((,(datum->syntax test 'it) ,test))
       (if ,(datum->syntax test 'it) ,then ,else)))

  (let ((it 999))
    (assert-eq (aif4 42 it 0) 42 "test_anaphoric_if_no_capture_of_outer_it")))

# datum->syntax with a symbol datum creates a binding visible at call site.
(begin
  (defmacro bind-as-x (val body)
    `(let ((,(datum->syntax val 'x) ,val)) ,body))

  (assert-eq (bind-as-x 100 (+ x 1)) 101 "test_datum_to_syntax_with_symbol"))

# When the context IS a syntax object (symbol argument), datum->syntax
# copies its scopes. The scope_exempt flag prevents the intro scope from
# being added, so the binding resolves correctly.
(begin
  (defmacro bind-it (name val body)
    `(let ((,(datum->syntax name 'it) ,val)) ,body))

  (assert-eq (bind-it x 42 (+ it 1)) 43 "test_datum_to_syntax_with_syntax_context"))

# datum->syntax with a list datum — set_scopes_recursive must recurse
# into the list structure, not just set scopes on the outer node.
(begin
  (defmacro inject-list (ctx)
    `(let ((,(datum->syntax ctx 'result) (list 1 2 3))) result))

  (assert-eq (inject-list x) (list 1 2 3) "test_datum_to_syntax_with_compound_datum"))

# SECTION 8: syntax->datum — scope stripping

# syntax->datum on a syntax object returns the plain value.
# Inside a macro, the argument is a syntax object; stripping it
# gives the underlying symbol/value.
(begin
  (defmacro get-datum (x)
    (syntax->datum x))

  (assert-eq (get-datum 42) 42 "test_syntax_to_datum_strips_scopes"))

# syntax->datum on a non-syntax value returns it unchanged.
(begin
  (assert-eq (syntax->datum 42) 42 "test_syntax_to_datum_non_syntax_passthrough"))
