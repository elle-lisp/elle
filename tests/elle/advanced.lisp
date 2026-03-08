## Advanced Runtime Features and Pattern Matching Tests
##
## Migrated from tests/integration/advanced.rs
## Tests import-file, spawn, join, sleep, debug-print, trace, memory-usage,
## pattern matching, guards, or-patterns, decision trees, and exhaustiveness.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Phase 5: Advanced Runtime Features - Integration Tests
# ============================================================================

# import-file tests
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "import-file with valid file succeeds")
(assert-err (fn () (import-file "./lib/nonexistent.lisp"))
  "import-file with non-existent relative path fails")
(assert-err (fn () (import-file "/absolute/nonexistent.lisp"))
  "import-file with non-existent absolute path fails")

# spawn and thread-id tests
(assert-true (int? (current-thread-id))
  "current-thread-id returns an integer")
(assert-true (> (current-thread-id) 0)
  "current-thread-id returns positive integer")

# debug-print tests
(assert-eq (debug-print 42) 42
  "debug-print returns the value (int)")
(assert-eq (debug-print "hello") "hello"
  "debug-print returns the value (string)")
(assert-eq (debug-print (+ 1 2)) 3
  "debug-print works with expressions")

# trace tests
(assert-eq (trace "label" 42) 42
  "trace returns the second argument")
(assert-eq (trace "computation" (+ 5 3)) 8
  "trace works with expressions")

# memory-usage test
(assert-true (fn () (let ((result (memory-usage)))
                      (or (list? result) (nil? result))))
  "memory-usage returns a list or nil")

# concurrency with arithmetic
(assert-true (int? (+ (current-thread-id) 1))
  "current-thread-id can be used in arithmetic")

# debug-print with list operations
(assert-eq (debug-print (list 1 2 3)) (list 1 2 3)
  "debug-print works with list operations")

# trace with arithmetic chain
(assert-eq (trace "step1" (+ 1 2)) 3
  "trace with first arithmetic")
(assert-eq (trace "step2" (* 3 4)) 12
  "trace with second arithmetic")

# multiple debug calls
(assert-eq (begin (debug-print 1) (debug-print 2) (debug-print 3)) 3
  "multiple debug-prints return last value")

# module and arithmetic combination
(assert-eq (+ 1 2) 3
  "arithmetic before import-file")
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "import-file succeeds")
(assert-eq (+ 1 2) 3
  "arithmetic after import-file")

# thread id consistency
(assert-eq (current-thread-id) (current-thread-id)
  "multiple calls to current-thread-id return same value")

# debug-print with nested structures
(assert-true (fn () (debug-print (list (list 1 2) (list 3 4))))
  "debug-print with nested lists")
(assert-true (fn () (debug-print (array 1 2 3)))
  "debug-print with arrays")

# phase 5 feature availability
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "import-file available")
(assert-true (fn () (spawn (fn () 42)))
  "spawn available")
(assert-true (fn () (join (spawn (fn () 42))))
  "join available")
(assert-true (fn () (time/sleep 0))
  "time/sleep available")
(assert-true (fn () (current-thread-id))
  "current-thread-id available")
(assert-true (fn () (debug-print 42))
  "debug-print available")
(assert-true (fn () (trace "x" 42))
  "trace available")
(assert-true (fn () (memory-usage))
  "memory-usage available")

# ============================================================================
# Error cases for Phase 5 features
# ============================================================================

# import-file wrong argument count
(assert-err (fn () (eval '(import-file)))
  "import-file with no arguments fails")
(assert-err (fn () (eval '(import-file "a" "b")))
  "import-file with two arguments fails")

# import-file wrong argument type
(assert-err (fn () (import-file 42))
  "import-file with int argument fails")
(assert-err (fn () (import-file nil))
  "import-file with nil argument fails")

# spawn wrong argument count
(assert-err (fn () (eval '(spawn)))
  "spawn with no arguments fails")
(assert-err (fn () (eval '(spawn + *)))
  "spawn with two arguments fails")

# spawn wrong argument type
(assert-err (fn () (spawn 42))
  "spawn with int argument fails")
(assert-err (fn () (spawn "not a function"))
  "spawn with string argument fails")

# join wrong argument count
(assert-err (fn () (eval '(join)))
  "join with no arguments fails")
(assert-err (fn () (eval '(join "a" "b")))
  "join with two arguments fails")

# sleep wrong argument count
(assert-err (fn () (eval '(time/sleep)))
  "time/sleep with no arguments fails")
(assert-err (fn () (eval '(time/sleep 1 2)))
  "time/sleep with two arguments fails")

# sleep wrong argument type
(assert-err (fn () (time/sleep "not a number"))
  "time/sleep with string argument fails")
(assert-err (fn () (time/sleep nil))
  "time/sleep with nil argument fails")

# sleep negative duration
(assert-err (fn () (time/sleep -1))
  "time/sleep with negative int fails")
(assert-err (fn () (time/sleep -0.5))
  "time/sleep with negative float fails")

# current-thread-id no arguments
(assert-true (fn () (current-thread-id))
  "current-thread-id with no arguments succeeds")

# debug-print wrong argument count
(assert-err (fn () (eval '(debug-print)))
  "debug-print with no arguments fails")
(assert-err (fn () (eval '(debug-print 1 2)))
  "debug-print with two arguments fails")

# trace wrong argument count
(assert-err (fn () (eval '(trace)))
  "trace with no arguments fails")
(assert-err (fn () (eval '(trace "label")))
  "trace with one argument fails")
(assert-err (fn () (eval '(trace "a" "b" "c")))
  "trace with three arguments fails")

# trace invalid label type
(assert-err (fn () (trace 42 100))
  "trace with int label fails")
(assert-err (fn () (trace nil 100))
  "trace with nil label fails")

# memory-usage no arguments
(assert-true (fn () (memory-usage))
  "memory-usage with no arguments succeeds")

# ============================================================================
# Pattern matching tests
# ============================================================================

# match syntax parsing
(assert-true (fn () (match 5 (5 "five") (_ nil)))
  "match syntax is properly parsed")

# match wildcard catches any
(assert-true (fn () (match 42 (_ "matched")))
  "wildcard matches int")
(assert-true (fn () (match "test" (_ true)))
  "wildcard matches string")

# match returns result expression
(assert-true (fn () (let ((v (match 5 (5 42) (10 0) (_ nil))))
                      (and (int? v) (> v 0))))
  "match returns positive number")

# match clause ordering
(assert-true (fn () (match 5 (5 true) (5 false) (_ nil)))
  "first matching clause is used")

# match default wildcard
(assert-true (fn () (match 99 (1 "one") (2 "two") (_ "other")))
  "wildcard matches when no literals match")

# match nil pattern parsing
(assert-true (fn () (match nil (nil "empty") (_ nil)))
  "nil pattern parses and works")

# match wildcard pattern
(assert-eq (match 42 (_ "any")) "any"
  "match wildcard with int")
(assert-eq (match "hello" (_ "matched")) "matched"
  "match wildcard with string")

# match nil pattern
(assert-eq (match nil (nil "empty") (_ nil)) "empty"
  "match nil pattern matches nil")
(assert-eq (match (list) (nil "empty") (_ "not-nil")) "not-nil"
  "nil pattern does not match empty list")

# match default case
(assert-eq (match 99 (1 "one") (2 "two") (_ "other")) "other"
  "default pattern catches unmatched values")

# match multiple clauses ordering
(assert-eq (match 2 (1 "one") (2 "two") (3 "three") (_ nil)) "two"
  "match clause ordering: 2 matches second clause")
(assert-eq (match 1 (1 "one") (2 "two") (3 "three") (_ nil)) "one"
  "match clause ordering: 1 matches first clause")

# match with static expressions
(assert-eq (match 10 (10 (* 2 3)) (_ nil)) 6
  "match evaluates result expression (multiply)")
(assert-eq (match 5 (5 (+ 1 1)) (_ nil)) 2
  "match evaluates result expression (add)")

# match string literals
(assert-eq (match "hello" ("hello" "matched") (_ "no")) "matched"
  "match string literals")

# ============================================================================
# Integration scenarios
# ============================================================================

# error in trace argument
(assert-err (fn () (trace "bad" (undefined-var)))
  "trace with undefined variable fails")

# debug and trace chain
(assert-true (fn () (trace "a" (debug-print (+ 1 2))))
  "debug-print and trace can be chained")

# sleep in arithmetic context
(assert-err (fn () (+ 1 (time/sleep 0)))
  "sleep result cannot be used in arithmetic")

# import-file returns last value
(assert-true (fn () (let ((result (import-file "test-modules/test.lisp")))
                      (list? result)))
  "import-file returns list")

# import-file with variable definitions
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "import-file with variable definitions")

# import multiple files sequentially
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "first import-file succeeds")
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "second import-file succeeds")

# import same file twice idempotent
(assert-true (fn () (let ((r1 (import-file "test-modules/test.lisp"))
                          (r2 (import-file "test-modules/test.lisp")))
                      (and (list? r1) (= r2 true))))
  "import-file idempotent: first returns list, second returns true")

# import-file with relative paths
(assert-true (fn () (import-file "./test-modules/test.lisp"))
  "import-file with ./ relative path")
(assert-true (fn () (import-file "test-modules/test.lisp"))
  "import-file with relative path")

# ============================================================================
# Array pattern matching tests
# ============================================================================

# match array literal
(assert-eq (match [1 2 3] ([1 2 3] "exact") (_ "no")) "exact"
  "match exact array literal")

# match array binding
(assert-eq (match [10 20] ([a b] (+ a b)) (_ 0)) 30
  "match array with binding")

# match array wrong length
(assert-eq (match [1 2] ([a b c] "three") ([a b] "two") (_ nil)) "two"
  "match array wrong length falls through")

# match array not array
(assert-eq (match 42 ([a b] "array") (_ "other")) "other"
  "match non-array falls through")

# match array empty
(assert-eq (match [] ([] "empty") (_ "other")) "empty"
  "match empty array")

# match array rest
(assert-eq (match [1 2 3 4] ([a & rest] (length rest)) (_ 0)) 3
  "match array with rest captures remaining")

# match array nested
(assert-eq (match [1 [2 3]] ([a [b c]] (+ a (+ b c))) (_ 0)) 6
  "match nested arrays")

# ============================================================================
# Guard (when) tests
# ============================================================================

# match guard basic
(assert-eq (match 5 (x when (> x 3) "big") (x "small")) "big"
  "guard passes when condition true")
(assert-eq (match 2 (x when (> x 3) "big") (x "small")) "small"
  "guard falls through when condition false")

# match guard with literal
(assert-eq (match 10 (10 when false "nope") (10 "yes") (_ nil)) "yes"
  "guard with literal falls through on false")

# ============================================================================
# Cons pattern tests
# ============================================================================

# match cons pattern
(assert-eq (match (cons 1 2) ((h . t) (+ h t)) (_ 0)) 3
  "match cons pattern")

# match cons not pair
(assert-eq (match 42 ((h . t) "pair") (_ "nope")) "nope"
  "match non-pair falls through")

# ============================================================================
# List rest pattern tests
# ============================================================================

# match list rest
(assert-eq (match (list 1 2 3) ((a & rest) a) (_ nil)) 1
  "match list rest captures first")

# match list exact length
(assert-eq (match (list 1 2 3) ((1 2) "two") ((1 2 3) "three") (_ nil)) "three"
  "match list exact length")

# ============================================================================
# Keyword pattern test
# ============================================================================

# match keyword literal
(assert-eq (match :foo (:foo "matched") (_ "no")) "matched"
  "match keyword literal matches")
(assert-eq (match :bar (:foo "matched") (_ "no")) "no"
  "match keyword literal doesn't match different keyword")

# ============================================================================
# Variable binding test
# ============================================================================

# match variable binding
(assert-eq (match 42 (x (+ x 1))) 43
  "match variable binding")

# ============================================================================
# Non-exhaustive match is a compile-time error
# ============================================================================

# match non-exhaustive is error
(assert-err (fn () (eval '(match 42 (1 "one") (2 "two"))))
  "non-exhaustive match is error")

# ============================================================================
# Variadic macro tests
# ============================================================================

# variadic macro basic
(assert-eq (begin (defmacro my-list (& items) `(list ,;items)) (my-list 1 2 3))
           (list 1 2 3)
  "variadic macro basic")

# variadic macro fixed and rest
(assert-eq (begin (defmacro my-add (first & rest) `(+ ,first ,;rest)) (my-add 1 2 3))
           6
  "variadic macro with fixed and rest")

# variadic macro empty rest
(assert-eq (begin (defmacro my-list (& items) `(list ,;items)) (my-list))
           (list)
  "variadic macro with empty rest")

# variadic macro arity error
(assert-err (fn () (eval '(begin (defmacro foo (a b & rest) `(list ,a ,b ,;rest)) (foo 1))))
  "variadic macro arity error")

# variadic macro when multi body
(assert-eq (begin (defmacro my-when (test & body) `(if ,test (begin ,;body) nil)) (my-when true 1 2 3))
           3
  "variadic macro with when and multi body")

# ============================================================================
# Match: improper list patterns (a b . c)
# ============================================================================

# match improper list pattern
(assert-eq (match (cons 1 (cons 2 3)) ((a b . c) (list a b c)) (_ :no))
           (list 1 2 3)
  "match improper list pattern")

# match improper list pattern longer
(assert-eq (match (list 1 2 3 4 5) ((a b c . d) (list a b c d)) (_ :no))
           (list 1 2 3 (list 4 5))
  "match improper list pattern longer")

# match improper list pattern exact
(assert-eq (match (cons 1 2) ((a . b) (list a b)) (_ :no))
           (list 1 2)
  "match improper list pattern exact")

# match improper list pattern too short
(assert-eq (match (list 1) ((a b . c) :matched) (_ :no))
           :no
  "match improper list pattern too short")

# ============================================================================
# Match: or-patterns (or 1 2 3)
# ============================================================================

# or pattern basic
(assert-eq (match 2 ((or 1 2 3) :small) (_ :big)) :small
  "or pattern basic match")

# or pattern no match
(assert-eq (match 5 ((or 1 2 3) :small) (_ :big)) :big
  "or pattern no match")

# or pattern keywords
(assert-eq (match :b ((or :a :b :c) :found) (_ :not)) :found
  "or pattern with keywords")

# or pattern with binding
(assert-eq (match (cons 1 2) ((or (x . _) (_ . x)) x) (_ 0)) 1
  "or pattern with binding")

# or pattern with binding second
(assert-eq (match 99 ((or (x . _) x) x) (_ 0)) 99
  "or pattern with binding second alternative")

# or pattern different bindings error
(assert-err (fn () (eval '(match 1 ((or (x . y) (x . _)) :ok) (_ :no))))
  "or pattern different bindings error")

# or pattern with guard
(assert-eq (match 2 ((or 1 2 3) when true :yes) (_ :no)) :yes
  "or pattern with guard")

# or pattern nested in cons
(assert-eq (match (cons 2 :x) (((or 1 2) . t) t) (_ :fail)) :x
  "or pattern nested in cons")

# or pattern two alternatives
(assert-eq (match :y ((or :x :y) :found) (_ :not)) :found
  "or pattern two alternatives")

# or pattern with nil
(assert-eq (match nil ((or nil 0) :empty) (_ :other)) :empty
  "or pattern with nil")

# or pattern in tuple
(assert-eq (match [2 :x] ([(or 1 2) y] y) (_ :fail)) :x
  "or pattern in tuple")

# ============================================================================
# Guard test coverage
# ============================================================================

# guard references pattern var
(assert-eq (match 10 (x when (> x 5) :big) (x :small)) :big
  "guard references pattern var: big")
(assert-eq (match 3 (x when (> x 5) :big) (x :small)) :small
  "guard references pattern var: small")

# guard fallthrough
(assert-eq (match 5 (x when false :never) (x :always)) :always
  "guard fallthrough")

# guard with cons
(assert-eq (match (cons 1 2) ((h . t) when (> h 0) (+ h t)) (_ 0)) 3
  "guard with cons")

# guard with list
(assert-eq (match (list 1 2 3) ((a b c) when (> (+ a b c) 5) :big) (_ :small)) :big
  "guard with list")

# guard with tuple
(assert-eq (match [1 2] ([a b] when (< a b) :ordered) (_ :no)) :ordered
  "guard with tuple")

# guard with struct
(assert-eq (match {:x 10 :y 20} ({:x x :y y} when (> y x) :valid) (_ :no)) :valid
  "guard with struct")

# guard with rest
(assert-eq (match (list 1 2 3) ((a & rest) when (> a 0) rest) (_ :fail))
           (list 2 3)
  "guard with rest")

# guard fallthrough to wildcard
(assert-eq (match 5 (x when false :a) (_ :fallback)) :fallback
  "guard fallthrough to wildcard")

# guard complex body
(assert-eq (match 10 (x when (> x 5) (let ((y (* x 2))) y)) (x x)) 20
  "guard complex body")

# guard no binding leak
(assert-eq (match 5 (x when false x) (y (+ y 1))) 6
  "guard no binding leak")

# guard middle arm matches
(assert-eq (match 5 (x when (> x 10) :big) (x when (> x 3) :medium) (x :small)) :medium
  "guard middle arm matches")

# or pattern guard outer var
(assert-eq (let ((threshold 3)) (match 2 ((or 1 2 3) when (< threshold 5) :yes) (_ :no)))
           :yes
  "or pattern guard outer var")

# or pattern binding guard
(assert-eq (match (cons 6 :x) ((or (a . _) (_ . a)) when (> a 5) :big) (_ :small))
           :big
  "or pattern binding guard")

# or pattern guard fallthrough
(assert-eq (match 2 ((or 1 2 3) when false :never) (_ :fallback)) :fallback
  "or pattern guard fallthrough")

# ============================================================================
# Exhaustiveness tests
# ============================================================================

# exhaustive match with wildcard
(assert-eq (match 42 (1 :one) (_ :other)) :other
  "exhaustive match with wildcard")

# exhaustive match with variable
(assert-eq (match 42 (1 :one) (x x)) 42
  "exhaustive match with variable")

# non-exhaustive match error
(assert-err (fn () (eval '(match 42 (1 :one) (2 :two))))
  "non-exhaustive match error")

# exhaustive match booleans
(assert-eq (match true (true :t) (false :f)) :t
  "exhaustive match booleans")

# exhaustive or pattern booleans
(assert-eq (match true ((or true false) :both)) :both
  "exhaustive or pattern booleans")

# non-exhaustive guard on last arm
(assert-err (fn () (eval '(match 42 (x when (> x 0) :pos))))
  "non-exhaustive guard on last arm")

# ============================================================================
# Decision tree specific tests
# ============================================================================

# decision tree shared prefix
(assert-eq (match (list 1 2 3)
              ((1 2 3) :exact)
              ((1 2 _) :prefix)
              (_ :other))
           :exact
  "decision tree shared prefix exact")

# decision tree shared prefix second arm
(assert-eq (match (list 1 2 4)
              ((1 2 3) :exact)
              ((1 2 _) :prefix)
              (_ :other))
           :prefix
  "decision tree shared prefix second arm")

# decision tree multiple constructors
(assert-eq (match (list 1 2)
              (nil :nil)
              ((h . t) :pair)
              (_ :other))
           :pair
  "decision tree multiple constructors")

# decision tree literal discrimination
(assert-eq (match :c
              (:a 1)
              (:b 2)
              (:c 3)
              (:d 4)
              (_ 0))
           3
  "decision tree literal discrimination")

# decision tree nested tuple match
(assert-eq (match [1 [2 3]]
              ([1 [2 3]] :exact)
              ([1 [2 _]] :partial)
              ([_ _] :any-pair)
              (_ :other))
           :exact
  "decision tree nested tuple match")

# decision tree guard fallthrough to next constructor
(assert-eq (match 5
              (5 when false :guarded)
              (5 :unguarded)
              (_ :default))
           :unguarded
  "decision tree guard fallthrough to next constructor")

# decision tree or pattern with shared body
(assert-eq (match :b
              ((or :a :b :c) :first-group)
              ((or :d :e :f) :second-group)
              (_ :other))
           :first-group
  "decision tree or pattern with shared body")

# decision tree struct key discrimination
(assert-eq (match {:type :circle :radius 5}
              ({:type :circle :radius r} r)
              ({:type :square :side s} s)
              (_ 0))
           5
  "decision tree struct key discrimination")

# decision tree struct key discrimination second
(assert-eq (match {:type :square :side 7}
              ({:type :circle :radius r} r)
              ({:type :square :side s} s)
              (_ 0))
           7
  "decision tree struct key discrimination second")

# decision tree deeply nested
(assert-eq (match (list 1 (list 2 (list 3)))
              ((1 (2 (3))) :deep)
              ((1 (2 _)) :medium)
              ((1 _) :shallow)
              (_ :none))
           :deep
  "decision tree deeply nested")

# decision tree match in loop
(def test-result (list))
(each i (list 1 2 3)
  (def test-result (cons (match i
                           (1 :one)
                           (2 :two)
                           (3 :three)
                           (_ :other))
                         test-result)))
(assert-eq (reverse test-result)
           (list :one :two :three)
  "decision tree match in loop")

# decision tree boolean exhaustive
(assert-eq (match false
              (true :yes)
              (false :no))
           :no
  "decision tree boolean exhaustive")

# decision tree or boolean exhaustive
(assert-eq (match true
              ((or true false) :bool))
           :bool
  "decision tree or boolean exhaustive")

# or pattern decision tree shared
(assert-eq (match (cons 1 :x) ((1 . t) t) ((2 . t) t) (((or 3 4) . t) t) (_ :fail))
           :x
  "or pattern decision tree shared")

# or pattern nested decision tree
(assert-eq (match [3 :y] ([(or 1 2 3) v] v) (_ :fail))
           :y
  "or pattern nested decision tree")

# or pattern guard decision tree
(assert-eq (match 5 ((or 1 2 3) when true :small) ((or 4 5 6) :medium) (_ :big))
           :medium
  "or pattern guard decision tree")
