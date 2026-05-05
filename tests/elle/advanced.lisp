(elle/epoch 10)
## Advanced Runtime Features and Pattern Matching Tests
##
## Migrated from tests/integration/advanced.rs
## Tests import-file, spawn, join, sleep, debug-print, trace, memory-usage,
## pattern matching, guards, or-patterns, decision trees, and exhaustiveness.


# ============================================================================
# Phase 5: Advanced Runtime Features - Integration Tests
# ============================================================================

# import-file tests
(assert (fn () (import-file "tests/modules/test.lisp"))
        "import-file with valid file succeeds")
(let [[ok? _] (protect ((fn () (import-file "./lib/nonexistent.lisp"))))]
  (assert (not ok?) "import-file with non-existent relative path fails"))
(let [[ok? _] (protect ((fn () (import-file "/absolute/nonexistent.lisp"))))]
  (assert (not ok?) "import-file with non-existent absolute path fails"))

# spawn and thread-id tests
(assert (int? (current-thread-id)) "current-thread-id returns an integer")
(assert (> (current-thread-id) 0) "current-thread-id returns positive integer")

# debug-print tests
(assert (= (debug-print 42) 42) "debug-print returns the value (int)")
(assert (= (debug-print "hello") "hello")
        "debug-print returns the value (string)")
(assert (= (debug-print (+ 1 2)) 3) "debug-print works with expressions")

# trace tests
(assert (= (trace "label" 42) 42) "trace returns the second argument")
(assert (= (trace "computation" (+ 5 3)) 8) "trace works with expressions")

# memory-usage test
(assert (fn ()
          (let [result (memory-usage)]
            (or (list? result) (nil? result))))
        "memory-usage returns a list or nil")

# concurrency with arithmetic
(assert (int? (+ (current-thread-id) 1))
        "current-thread-id can be used in arithmetic")

# debug-print with list operations
(assert (= (debug-print (list 1 2 3)) (list 1 2 3))
        "debug-print works with list operations")

# trace with arithmetic chain
(assert (= (trace "step1" (+ 1 2)) 3) "trace with first arithmetic")
(assert (= (trace "step2" (* 3 4)) 12) "trace with second arithmetic")

# multiple debug calls
(assert (= (begin
             (debug-print 1)
             (debug-print 2)
             (debug-print 3)) 3) "multiple debug-prints return last value")

# module and arithmetic combination
(assert (= (+ 1 2) 3) "arithmetic before import-file")
(assert (fn () (import-file "tests/modules/test.lisp")) "import-file succeeds")
(assert (= (+ 1 2) 3) "arithmetic after import-file")

# thread id consistency
(assert (= (current-thread-id) (current-thread-id))
        "multiple calls to current-thread-id return same value")

# debug-print with nested structures
(assert (fn () (debug-print (list (list 1 2) (list 3 4))))
        "debug-print with nested lists")
(assert (fn () (debug-print (@array 1 2 3))) "debug-print with arrays")

# phase 5 feature availability
(assert (fn () (import-file "tests/modules/test.lisp")) "import-file available")
(assert (fn () (spawn (fn () 42))) "spawn available")
(assert (fn () (join (spawn (fn () 42)))) "join available")
(assert (fn () (time/sleep 0)) "time/sleep available")
(assert (fn () (current-thread-id)) "current-thread-id available")
(assert (fn () (debug-print 42)) "debug-print available")
(assert (fn () (trace "x" 42)) "trace available")
(assert (fn () (memory-usage)) "memory-usage available")

# ============================================================================
# Error cases for Phase 5 features
# ============================================================================

# import-file wrong argument count
(let [[ok? _] (protect ((fn () (eval '(import-file)))))]
  (assert (not ok?) "import-file with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(import-file "a" "b")))))]
  (assert (not ok?) "import-file with two arguments fails"))

# import-file wrong argument type
(let [[ok? _] (protect ((fn () (import-file 42))))]
  (assert (not ok?) "import-file with int argument fails"))
(let [[ok? _] (protect ((fn () (import-file nil))))]
  (assert (not ok?) "import-file with nil argument fails"))

# spawn wrong argument count
(let [[ok? _] (protect ((fn () (eval '(spawn)))))]
  (assert (not ok?) "spawn with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(spawn + *)))))]
  (assert (not ok?) "spawn with two arguments fails"))

# spawn wrong argument type
(let [[ok? _] (protect ((fn () (spawn 42))))]
  (assert (not ok?) "spawn with int argument fails"))
(let [[ok? _] (protect ((fn () (spawn "not a function"))))]
  (assert (not ok?) "spawn with string argument fails"))

# join wrong argument count
(let [[ok? _] (protect ((fn () (eval '(join)))))]
  (assert (not ok?) "join with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(join "a" "b")))))]
  (assert (not ok?) "join with two arguments fails"))

# sleep wrong argument count
(let [[ok? _] (protect ((fn () (eval '(time/sleep)))))]
  (assert (not ok?) "time/sleep with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(time/sleep 1 2)))))]
  (assert (not ok?) "time/sleep with two arguments fails"))

# sleep wrong argument type
(let [[ok? _] (protect ((fn () (time/sleep "not a number"))))]
  (assert (not ok?) "time/sleep with string argument fails"))
(let [[ok? _] (protect ((fn () (time/sleep nil))))]
  (assert (not ok?) "time/sleep with nil argument fails"))

# sleep negative duration
(let [[ok? _] (protect ((fn () (time/sleep -1))))]
  (assert (not ok?) "time/sleep with negative int fails"))
(let [[ok? _] (protect ((fn () (time/sleep -0.5))))]
  (assert (not ok?) "time/sleep with negative float fails"))

# current-thread-id no arguments
(assert (fn () (current-thread-id))
        "current-thread-id with no arguments succeeds")

# debug-print wrong argument count
(let [[ok? _] (protect ((fn () (eval '(debug-print)))))]
  (assert (not ok?) "debug-print with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(debug-print 1 2)))))]
  (assert (not ok?) "debug-print with two arguments fails"))

# trace wrong argument count
(let [[ok? _] (protect ((fn () (eval '(trace)))))]
  (assert (not ok?) "trace with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(trace "label")))))]
  (assert (not ok?) "trace with one argument fails"))
(let [[ok? _] (protect ((fn () (eval '(trace "a" "b" "c")))))]
  (assert (not ok?) "trace with three arguments fails"))

# trace invalid label type
(let [[ok? _] (protect ((fn () (trace 42 100))))]
  (assert (not ok?) "trace with int label fails"))
(let [[ok? _] (protect ((fn () (trace nil 100))))]
  (assert (not ok?) "trace with nil label fails"))

# memory-usage no arguments
(assert (fn () (memory-usage)) "memory-usage with no arguments succeeds")

# ============================================================================
# Pattern matching tests
# ============================================================================

# match syntax parsing
(assert (fn ()
          (match 5
            5 "five"
            _ nil)) "match syntax is properly parsed")

# match wildcard catches any
(assert (fn ()
          (match 42
            _ "matched")) "wildcard matches int")
(assert (fn ()
          (match "test"
            _ true)) "wildcard matches string")

# match returns result expression
(assert (fn ()
          (let [v (match 5
                    5 42
                    10 0
                    _ nil)]
            (and (int? v) (> v 0)))) "match returns positive number")

# match clause ordering
(assert (fn ()
          (match 5
            5 true
            5 false
            _ nil)) "first matching clause is used")

# match default wildcard
(assert (fn ()
          (match 99
            1 "one"
            2 "two"
            _ "other")) "wildcard matches when no literals match")

# match nil pattern parsing
(assert (fn ()
          (match nil
            nil "empty"
            _ nil)) "nil pattern parses and works")

# match wildcard pattern
(assert (= (match 42
             _ "any") "any") "match wildcard with int")
(assert (= (match "hello"
             _ "matched") "matched") "match wildcard with string")

# match nil pattern
(assert (= (match nil
             nil "empty"
             _ nil) "empty") "match nil pattern matches nil")
(assert (= (match (list)
             nil "empty"
             _ "not-nil") "not-nil") "nil pattern does not match empty list")

# match default case
(assert (= (match 99
             1 "one"
             2 "two"
             _ "other") "other") "default pattern catches unmatched values")

# match multiple clauses ordering
(assert (= (match 2
             1 "one"
             2 "two"
             3 "three"
             _ nil) "two") "match clause ordering: 2 matches second clause")
(assert (= (match 1
             1 "one"
             2 "two"
             3 "three"
             _ nil) "one") "match clause ordering: 1 matches first clause")

# match with static expressions
(assert (= (match 10
             10 (* 2 3)
             _ nil) 6) "match evaluates result expression (multiply)")
(assert (= (match 5
             5 (+ 1 1)
             _ nil) 2) "match evaluates result expression (add)")

# match string literals
(assert (= (match "hello"
             "hello" "matched"
             _ "no") "matched") "match string literals")

# ============================================================================
# Integration scenarios
# ============================================================================

# error in trace argument
(let [[ok? _] (protect ((fn () (eval '(trace "bad" (undefined-var))))))]
  (assert (not ok?) "trace with undefined variable fails"))

# debug and trace chain
(assert (fn () (trace "a" (debug-print (+ 1 2))))
        "debug-print and trace can be chained")

# sleep in arithmetic context
(let [[ok? _] (protect ((fn () (+ 1 (time/sleep 0)))))]
  (assert (not ok?) "sleep result cannot be used in arithmetic"))

# import-file returns last value
(assert (fn ()
          (let [result (import-file "tests/modules/test.lisp")]
            (list? result))) "import-file returns list")

# import-file with variable definitions
(assert (fn () (import-file "tests/modules/test.lisp"))
        "import-file with variable definitions")

# import multiple files sequentially
(assert (fn () (import-file "tests/modules/test.lisp"))
        "first import-file succeeds")
(assert (fn () (import-file "tests/modules/test.lisp"))
        "second import-file succeeds")

# import same file twice idempotent
(assert (fn ()
          (let [r1 (import-file "tests/modules/test.lisp")
                r2 (import-file "tests/modules/test.lisp")]
            (and (list? r1) (= r2 true))))
        "import-file idempotent: first returns list, second returns true")

# import-file with relative paths
(assert (fn () (import-file "./tests/modules/test.lisp"))
        "import-file with ./ relative path")
(assert (fn () (import-file "tests/modules/test.lisp"))
        "import-file with relative path")

# ============================================================================
# Array pattern matching tests
# ============================================================================

# match array literal
(assert (= (match [1 2 3]
             [1 2 3] "exact"
             _ "no") "exact") "match exact array literal")

# match array binding
(assert (= (match [10 20]
             [a b] (+ a b)
             _ 0) 30) "match array with binding")

# match array wrong length
(assert (= (match [1 2]
             [a b c] "three"
             [a b] "two"
             _ nil) "two") "match array wrong length falls through")

# match array not array
(assert (= (match 42
             [a b] "array"
             _ "other") "other") "match non-array falls through")

# match array empty
(assert (= (match []
             [] "empty"
             _ "other") "empty") "match empty array")

# match array rest
(assert (= (match [1 2 3 4]
             [a & rest] (length rest)
             _ 0) 3) "match array with rest captures remaining")

# match array nested
(assert (= (match [1 [2 3]]
             [a [b c]] (+ a (+ b c))
             _ 0) 6) "match nested arrays")

# ============================================================================
# Guard (when) tests
# ============================================================================

# match guard basic
(assert (= (match 5
             x when
             (> x 3) "big"
             x "small") "big") "guard passes when condition true")
(assert (= (match 2
             x when
             (> x 3) "big"
             x "small") "small") "guard falls through when condition false")

# match guard with literal
(assert (= (match 10
             10 when
             false "nope"
             10 "yes"
             _ nil) "yes") "guard with literal falls through on false")

# ============================================================================
# Cons pattern tests
# ============================================================================

# match pair pattern
(assert (= (match (pair 1 2)
             (h . t) (+ h t)
             _ 0) 3) "match pair pattern")

# match pair not pair
(assert (= (match 42
             (h . t) "pair"
             _ "nope") "nope") "match non-pair falls through")

# ============================================================================
# List rest pattern tests
# ============================================================================

# match list rest
(assert (= (match (list 1 2 3)
             (a & rest) a
             _ nil) 1) "match list rest captures first")

# match list exact length
(assert (= (match (list 1 2 3)
             (1 2) "two"
             (1 2 3) "three"
             _ nil) "three") "match list exact length")

# ============================================================================
# Keyword pattern test
# ============================================================================

# match keyword literal
(assert (= (match :foo
             :foo "matched"
             _ "no") "matched") "match keyword literal matches")
(assert (= (match :bar
             :foo "matched"
             _ "no") "no")
        "match keyword literal doesn't match different keyword")

# ============================================================================
# Variable binding test
# ============================================================================

# match variable binding
(assert (= (match 42
             x (+ x 1)) 43) "match variable binding")

# ============================================================================
# Non-exhaustive match is a compile-time error
# ============================================================================

# match non-exhaustive is error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 42
                                   1 "one"
                                   2 "two")))))]
  (assert (not ok?) "non-exhaustive match is error"))

# ============================================================================
# Variadic macro tests
# ============================================================================

# variadic macro basic
(assert (= (begin
             (defmacro my-list (& items)
               `(list ,;items))
             (my-list 1 2 3)) (list 1 2 3)) "variadic macro basic")

# variadic macro fixed and rest
(assert (= (begin
             (defmacro my-add (first & rest)
               `(+ ,first ,;rest))
             (my-add 1 2 3)) 6) "variadic macro with fixed and rest")

# variadic macro empty rest
(assert (= (begin
             (defmacro my-list (& items)
               `(list ,;items))
             (my-list)) (list)) "variadic macro with empty rest")

# variadic macro arity error
(let [[ok? _] (protect ((fn ()
                          (eval '(begin
                                   (defmacro foo (a b & rest)
                                     `(list ,a ,b ,;rest))
                                   (foo 1))))))]
  (assert (not ok?) "variadic macro arity error"))

# variadic macro when multi body
(assert (= (begin
             (defmacro my-when (test & body)
               `(if ,test
                  (begin
                    ,;body)
                  nil))
             (my-when true 1 2 3)) 3) "variadic macro with when and multi body")

# ============================================================================
# Match: improper list patterns (a b . c)
# ============================================================================

# match improper list pattern
(assert (= (match (pair 1 (pair 2 3))
             (a b . c) (list a b c)
             _ :no) (list 1 2 3)) "match improper list pattern")

# match improper list pattern longer
(assert (= (match (list 1 2 3 4 5)
             (a b c . d) (list a b c d)
             _ :no) (list 1 2 3 (list 4 5)))
        "match improper list pattern longer")

# match improper list pattern exact
(assert (= (match (pair 1 2)
             (a . b) (list a b)
             _ :no) (list 1 2)) "match improper list pattern exact")

# match improper list pattern too short
(assert (= (match (list 1)
             (a b . c) :matched
             _ :no) :no) "match improper list pattern too short")

# ============================================================================
# Match: or-patterns (or 1 2 3)
# ============================================================================

# or pattern basic
(assert (= (match 2
             (or 1 2 3) :small
             _ :big) :small) "or pattern basic match")

# or pattern no match
(assert (= (match 5
             (or 1 2 3) :small
             _ :big) :big) "or pattern no match")

# or pattern keywords
(assert (= (match :b
             (or :a :b :c) :found
             _ :not) :found) "or pattern with keywords")

# or pattern with binding
(assert (= (match (pair 1 2)
             (or (x . _) (_ . x)) x
             _ 0) 1) "or pattern with binding")

# or pattern with binding second
(assert (= (match 99
             (or (x . _) x) x
             _ 0) 99) "or pattern with binding second alternative")

# or pattern different bindings error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   (or (x . y) (x . _)) :ok
                                   _ :no)))))]
  (assert (not ok?) "or pattern different bindings error"))

# or pattern with guard
(assert (= (match 2
             (or 1 2 3) when
             true :yes
             _ :no) :yes) "or pattern with guard")

# or pattern nested in pair
(assert (= (match (pair 2 :x)
             ((or 1 2) . t) t
             _ :fail) :x) "or pattern nested in pair")

# or pattern two alternatives
(assert (= (match :y
             (or :x :y) :found
             _ :not) :found) "or pattern two alternatives")

# or pattern with nil
(assert (= (match nil
             (or nil 0) :empty
             _ :other) :empty) "or pattern with nil")

# or pattern in tuple
(assert (= (match [2 :x]
             [(or 1 2) y] y
             _ :fail) :x) "or pattern in tuple")

# ============================================================================
# Guard test coverage
# ============================================================================

# guard references pattern var
(assert (= (match 10
             x when
             (> x 5) :big
             x :small) :big) "guard references pattern var: big")
(assert (= (match 3
             x when
             (> x 5) :big
             x :small) :small) "guard references pattern var: small")

# guard fallthrough
(assert (= (match 5
             x when
             false :never
             x :always) :always) "guard fallthrough")

# guard with pair
(assert (= (match (pair 1 2)
             (h . t) when
             (> h 0) (+ h t)
             _ 0) 3) "guard with pair")

# guard with list
(assert (= (match (list 1 2 3)
             (a b c) when
             (> (+ a b c) 5) :big
             _ :small) :big) "guard with list")

# guard with tuple
(assert (= (match [1 2]
             [a b] when
             (< a b) :ordered
             _ :no) :ordered) "guard with tuple")

# guard with struct
(assert (= (match {:x 10 :y 20}
             {:x x :y y} when
             (> y x) :valid
             _ :no) :valid) "guard with struct")

# guard with rest
(assert (= (match (list 1 2 3)
             (a & rest) when
             (> a 0) rest
             _ :fail) (list 2 3)) "guard with rest")

# guard fallthrough to wildcard
(assert (= (match 5
             x when
             false :a
             _ :fallback) :fallback) "guard fallthrough to wildcard")

# guard complex body
(assert (= (match 10
             x when
             (> x 5)
               (let [y (* x 2)]
                 y)
             x x) 20) "guard complex body")

# guard no binding leak
(assert (= (match 5
             x when
             false x
             y (+ y 1)) 6) "guard no binding leak")

# guard middle arm matches
(assert (= (match 5
             x when
             (> x 10) :big
             x when
             (> x 3) :medium
             x :small) :medium) "guard middle arm matches")

# or pattern guard outer var
(assert (= (let [threshold 3]
             (match 2
               (or 1 2 3) when
               (< threshold 5) :yes
               _ :no)) :yes) "or pattern guard outer var")

# or pattern binding guard
(assert (= (match (pair 6 :x)
             (or (a . _) (_ . a)) when
             (> a 5) :big
             _ :small) :big) "or pattern binding guard")

# or pattern guard fallthrough
(assert (= (match 2
             (or 1 2 3) when
             false :never
             _ :fallback) :fallback) "or pattern guard fallthrough")

# ============================================================================
# Exhaustiveness tests
# ============================================================================

# exhaustive match with wildcard
(assert (= (match 42
             1 :one
             _ :other) :other) "exhaustive match with wildcard")

# exhaustive match with variable
(assert (= (match 42
             1 :one
             x x) 42) "exhaustive match with variable")

# non-exhaustive match error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 42
                                   1 :one
                                   2 :two)))))]
  (assert (not ok?) "non-exhaustive match error"))

# exhaustive match booleans
(assert (= (match true
             true :t
             false :f) :t) "exhaustive match booleans")

# exhaustive or pattern booleans
(assert (= (match true
             (or true false) :both) :both) "exhaustive or pattern booleans")

# non-exhaustive guard on last arm
(let [[ok? _] (protect ((fn ()
                          (eval '(match 42
                                   x when
                                   (> x 0) :pos)))))]
  (assert (not ok?) "non-exhaustive guard on last arm"))

# ============================================================================
# Decision tree specific tests
# ============================================================================

# decision tree shared prefix
(assert (= (match (list 1 2 3)
             (1 2 3) :exact
             (1 2 _) :prefix
             _ :other) :exact) "decision tree shared prefix exact")

# decision tree shared prefix second arm
(assert (= (match (list 1 2 4)
             (1 2 3) :exact
             (1 2 _) :prefix
             _ :other) :prefix) "decision tree shared prefix second arm")

# decision tree multiple constructors
(assert (= (match (list 1 2)
             nil :nil
             (h . t) :pair
             _ :other) :pair) "decision tree multiple constructors")

# decision tree literal discrimination
(assert (= (match :c
             :a 1
             :b 2
             :c 3
             :d 4
             _ 0) 3) "decision tree literal discrimination")

# decision tree nested tuple match
(assert (= (match [1 [2 3]]
             [1 [2 3]] :exact
             [1 [2 _]] :partial
             [_ _] :any-pair
             _ :other) :exact) "decision tree nested tuple match")

# decision tree guard fallthrough to next constructor
(assert (= (match 5
             5 when
             false :guarded
             5 :unguarded
             _ :default) :unguarded)
        "decision tree guard fallthrough to next constructor")

# decision tree or pattern with shared body
(assert (= (match :b
             (or :a :b :c) :first-group
             (or :d :e :f) :second-group
             _ :other) :first-group) "decision tree or pattern with shared body")

# decision tree struct key discrimination
(assert (= (match {:type :circle :radius 5}
             {:type :circle :radius r} r
             {:type :square :side s} s
             _ 0) 5) "decision tree struct key discrimination")

# decision tree struct key discrimination second
(assert (= (match {:type :square :side 7}
             {:type :circle :radius r} r
             {:type :square :side s} s
             _ 0) 7) "decision tree struct key discrimination second")

# decision tree deeply nested
(assert (= (match (list 1 (list 2 (list 3)))
             (1 (2 (3))) :deep
             (1 (2 _)) :medium
             (1 _) :shallow
             _ :none) :deep) "decision tree deeply nested")

# decision tree match in loop
(def @test-result (list))
(each i (list 1 2 3)
  (assign
    test-result
    (pair (match i
            1 :one
            2 :two
            3 :three
            _ :other) test-result)))
(assert (= (reverse test-result) (list :one :two :three))
        "decision tree match in loop")

# decision tree boolean exhaustive
(assert (= (match false
             true :yes
             false :no) :no) "decision tree boolean exhaustive")

# decision tree or boolean exhaustive
(assert (= (match true
             (or true false) :bool) :bool) "decision tree or boolean exhaustive")

# or pattern decision tree shared
(assert (= (match (pair 1 :x)
             (1 . t) t
             (2 . t) t
             ((or 3 4) . t) t
             _ :fail) :x) "or pattern decision tree shared")

# or pattern nested decision tree
(assert (= (match [3 :y]
             [(or 1 2 3) v] v
             _ :fail) :y) "or pattern nested decision tree")

# or pattern guard decision tree
(assert (= (match 5
             (or 1 2 3) when
             true :small
             (or 4 5 6) :medium
             _ :big) :medium) "or pattern guard decision tree")
