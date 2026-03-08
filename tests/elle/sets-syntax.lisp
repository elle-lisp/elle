## Set Literal Syntax Tests
##
## Tests the syntax and parsing of set literals (chunk 2-3 of issue #509).
## Exercises the reader/parser layer for set literals and or-patterns.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Set literal syntax — immutable sets
# ============================================================================

(assert-eq (type |1 2 3|) :set
  "set literal has type :set")

(assert-eq (length |1 2 3|) 3
  "set literal has 3 elements")

(assert-eq (length ||) 0
  "empty set literal")

(assert-eq |1 2 3| |1 2 3|
  "set literals are equal")

(assert-eq |3 1 2| |1 2 3|
  "set order doesn't matter (both are equal)")

(assert-eq (length |1 1 2|) 2
  "set deduplicates elements")

# ============================================================================
# Set literal syntax — mutable sets
# ============================================================================

(assert-eq (type @|1 2 3|) :@set
  "mutable set literal has type :@set")

(assert-eq (length @|1 2 3|) 3
  "mutable set literal has 3 elements")

(assert-eq (length @||) 0
  "empty mutable set literal")

(assert-eq @|1 2 3| @|1 2 3|
  "mutable set literals are equal")

(assert-eq @|3 1 2| @|1 2 3|
  "mutable set order doesn't matter (both are equal)")

(assert-eq (length @|1 1 2|) 2
  "mutable set deduplicates elements")

# ============================================================================
# Set predicate covers both types
# ============================================================================

(assert-true (set? |1 2 3|)
  "set? true for immutable set")

(assert-true (set? @|1 2 3|)
  "set? true for mutable set")

(assert-false (set? [1 2 3])
  "set? false for tuple")

(assert-false (set? (list 1 2 3))
  "set? false for list")

(assert-false (set? "hello")
  "set? false for string")

# ============================================================================
# :@keyword syntax
# ============================================================================

(assert-eq :@set :@set
  ":@set is a valid keyword")

(assert-true (keyword? :@set)
  ":@set is a keyword")

# ============================================================================
# Multiple match arms (or-patterns with | delimiter now conflict with sets)
# ============================================================================

(assert-eq (match 1 (1 :odd) (3 :odd) (5 :odd) (_ :even)) :odd
  "match with multiple arms: 1 is odd")

(assert-eq (match 2 (1 :odd) (3 :odd) (5 :odd) (_ :even)) :even
  "match with multiple arms: 2 is even")

(assert-eq (match 3 (1 :odd) (3 :odd) (5 :odd) (_ :even)) :odd
  "match with multiple arms: 3 is odd")

(assert-eq (match 5 (1 :odd) (3 :odd) (5 :odd) (_ :even)) :odd
  "match with multiple arms: 5 is odd")

(assert-eq (match 4 (1 :odd) (3 :odd) (5 :odd) (_ :even)) :even
  "match with multiple arms: 4 is even")

# ============================================================================
# Set inside a list (set as expression)
# ============================================================================

(def s |1 2 3|)
(assert-true (set? s)
  "set assigned to variable")

(assert-eq (length s) 3
  "set variable has correct length")

# ============================================================================
# Nested sets (via constructor — |...| can't nest due to delimiter ambiguity)
# ============================================================================

(assert-eq (length (set |1 2|)) 1
  "set containing a set")

(assert-true (set? (first (set->list (set |1 2|))))
  "nested set is a set")

# ============================================================================
# Mixed immutable and mutable sets
# ============================================================================

(assert-false (= |1 2 3| @|1 2 3|)
  "immutable and mutable sets are not equal")

(assert-true (set? |1 2 3|)
  "immutable set passes set? predicate")

(assert-true (set? @|1 2 3|)
  "mutable set passes set? predicate")

# ============================================================================
# Set literals in various contexts
# ============================================================================

(assert-eq (+ 1 (length |1 2 3|)) 4
  "set literal in call position")

(def my-set |10 20 30|)
(assert-eq (length my-set) 3
  "set literal assigned to variable")

(assert-eq (length (list |1| |2| |3|)) 3
  "set literals in list")

# ============================================================================
# Empty sets
# ============================================================================

(assert-eq || ||
  "empty immutable sets are equal")

(assert-eq @|| @||
  "empty mutable sets are equal")

(assert-eq (length ||) 0
  "empty immutable set has length 0")

(assert-eq (length @||) 0
  "empty mutable set has length 0")

(assert-true (empty? ||)
  "empty immutable set is empty?")

(assert-true (empty? @||)
  "empty mutable set is empty?")
