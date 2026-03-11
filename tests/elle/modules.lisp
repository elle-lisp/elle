# Module system — parametric modules, qualified symbols, selective import

# ============================================================================
# 1. Basic parametric import with qualified symbol access
# ============================================================================

(let ([fmt ((import-file "tests/modules/formatter.lisp") :prefix "[" :suffix "]" :separator " | ")])
  (assert (= (fmt:wrap "hello") "[hello]") "qualified access: wrap with prefix/suffix")
  (assert (= (fmt:join [1 2 3]) "1 | 2 | 3") "qualified access: join with separator")
  (assert (= (fmt:upper "hello") "HELLO") "qualified access: upper (unconfigured)")
  (assert (= (fmt:identity 42) 42) "qualified access: identity"))

# ============================================================================
# 2. Two instances with different configurations
# ============================================================================

(let ([brackets ((import-file "tests/modules/formatter.lisp") :prefix "(" :suffix ")")]
      [angles   ((import-file "tests/modules/formatter.lisp") :prefix "<" :suffix ">")])
  (assert (= (brackets:wrap "x") "(x)") "two instances: brackets wrap")
  (assert (= (angles:wrap "x") "<x>") "two instances: angles wrap")
  # Each instance has its own separator config
  (assert (= (brackets:join ["a" "b"]) "a, b") "two instances: brackets default separator")
  (assert (= (angles:join ["a" "b"]) "a, b") "two instances: angles default separator"))

# ============================================================================
# 3. Default parameters (no keyword args)
# ============================================================================

(let ([fmt ((import-file "tests/modules/formatter.lisp"))])
  (assert (= (fmt:wrap "hello") "hello") "defaults: wrap with empty prefix/suffix")
  (assert (= (fmt:join ["a" "b" "c"]) "a, b, c") "defaults: join with default separator"))

# ============================================================================
# 4. Selective destructuring import
# ============================================================================

(let ([{:wrap wrap :upper upper} ((import-file "tests/modules/formatter.lisp") :prefix "<<" :suffix ">>")])
  (assert (= (wrap "hi") "<<hi>>") "destructured: wrap")
  (assert (= (upper "hi") "HI") "destructured: upper"))

# ============================================================================
# 5. Module as first-class value
# ============================================================================

(defn apply-wrap [mod s]
  "Call wrap from a module struct."
  (mod:wrap s))

(let ([fmt ((import-file "tests/modules/formatter.lisp") :prefix "{" :suffix "}")])
  (assert (= (apply-wrap fmt "val") "{val}") "first-class: pass module to function"))

# ============================================================================
# 6. Existing module fixtures
# ============================================================================

# test.lisp — simple value exports
(let ([test-mod ((import-file "tests/modules/test.lisp"))])
  (assert (= test-mod:test-var 42) "test.lisp: test-var")
  (assert (= test-mod:test-string "hello") "test.lisp: test-string")
  (assert (= test-mod:test-list (list 1 2 3)) "test.lisp: test-list"))

# counter.lisp — stateful module (import-file re-executes, giving independent state)
(let ([c1 ((import-file "tests/modules/counter.lisp"))]
      [c2 ((import-file "tests/modules/counter.lisp"))])
  (c1:inc)
  (c1:inc)
  (c1:inc)
  (assert (= (c1:count) 3) "counter: c1 incremented three times")
  (assert (= (c2:count) 0) "counter: c2 independent, still zero")
  (c2:inc)
  (assert (= (c2:count) 1) "counter: c2 incremented once"))
