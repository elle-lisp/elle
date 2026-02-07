;; Pattern Matching Examples in Elle
;; Demonstrates basic pattern matching capabilities

;; Example 1: Basic literal matching
(match 42
  (1 "one")
  (2 "two")
  (42 "the answer"))

;; Example 2: String matching
(match "hello"
  ("hello" "greeting")
  ("goodbye" "farewell"))

;; Example 3: Wildcard pattern
(match 100
  (50 "fifty")
  (_ "something else"))

;; Example 4: Nil pattern
(match nil
  (nil "it's nil")
  (_ "not nil"))

;; Example 5: List matching
(match (list 1 2 3)
  ((1 2 3) "exact match")
  (_ "no match"))

;; Example 6: Empty list
(match (list)
  (nil "empty list")
  (_ "not empty"))

;; Example 7: Nested lists
(match (list (list 1 2) (list 3 4))
  (((1 2) (3 4)) "matched nested")
  (_ "no match"))

;; Example 8: Match with computed result
(match 100
  (50 (+ 50 50))
  (100 (+ 100 100))
  (_ 0))

;; Example 9: Multiple patterns
(match "foo"
  ("bar" "bar")
  ("baz" "baz")
  ("foo" "foo"))

;; Example 10: Complex list pattern
(match (list "name" "Alice" 30)
  (("name" "Alice" 30) "name Alice age 30")
  (_ "no match"))
