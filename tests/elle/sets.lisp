## Set types test suite
##
## Comprehensive tests for set types (immutable and mutable).
## Covers syntax, value creation, operations, and integration with other language features.
## Issue #509: Set type implementation.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Set Literal Syntax — Immutable Sets
# ============================================================================

(assert-eq (type (set 1 2 3)) :set
  "set literal has type :set")

(assert-eq (length (set 1 2 3)) 3
  "set literal has 3 elements")

(assert-eq (length ||) 0
  "empty set literal")

(assert-eq (set 1 2 3) (set 1 2 3)
  "set literals are equal")

(assert-eq (set 3 1 2) (set 1 2 3)
  "set order doesn't matter (both are equal)")

(assert-eq (length (set 1 1 2)) 2
  "set deduplicates elements")

# ============================================================================
# Set Literal Syntax — Mutable Sets
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
# Set Predicate Covers Both Types
# ============================================================================

(assert-true (set? (set 1 2 3))
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
# :@keyword Syntax
# ============================================================================

(assert-eq :@set :@set
  ":@set is a valid keyword")

(assert-true (keyword? :@set)
  ":@set is a keyword")

# ============================================================================
# Or-Patterns
# ============================================================================

(assert-eq (match 1 ((or 1 3 5) :odd) (_ :even)) :odd
  "or-pattern: 1 is odd")

(assert-eq (match 2 ((or 1 3 5) :odd) (_ :even)) :even
  "or-pattern: 2 is even")

(assert-eq (match 3 ((or 1 3 5) :odd) (_ :even)) :odd
  "or-pattern: 3 is odd")

(assert-eq (match 5 ((or 1 3 5) :odd) (_ :even)) :odd
  "or-pattern: 5 is odd")

(assert-eq (match 4 ((or 1 3 5) :odd) (_ :even)) :even
  "or-pattern: 4 is even")

# ============================================================================
# Set Inside a List (Set as Expression via Constructor)
# ============================================================================

(def s (set 1 2 3))
(assert-true (set? s)
  "set assigned to variable")

(assert-eq (length s) 3
  "set variable has correct length")

# ============================================================================
# Nested Sets (via Constructor)
# ============================================================================

(assert-eq (length (set (set 1 2))) 1
  "set containing a set")

(assert-true (set? (get (set->array (set (set 1 2))) 0))
  "nested set is a set")

# ============================================================================
# Mixed Immutable and Mutable Sets
# ============================================================================

(assert-false (= (set 1 2 3) @|1 2 3|)
  "immutable and mutable sets are not equal")

(assert-true (set? (set 1 2 3))
  "immutable set passes set? predicate")

(assert-true (set? @|1 2 3|)
  "mutable set passes set? predicate")

# ============================================================================
# Set Literals in Various Contexts
# ============================================================================

(assert-eq (+ 1 (length (set 1 2 3))) 4
  "set literal in call position")

(def my-set (set 10 20 30))
(assert-eq (length my-set) 3
  "set literal assigned to variable")

(assert-eq (length (list (set 1) (set 2) (set 3))) 3
  "set literals in list")

# ============================================================================
# Empty Sets
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

# ============================================================================
# Set Construction and Display
# ============================================================================

(assert-true (set? (set 1 2 3))
  "set constructor creates a set")

(assert-true (set? (@set 1 2 3))
  "@set constructor creates a mutable set")

(assert-eq (type (set 1 2 3)) :set
  "set constructor produces :set type")

(assert-eq (type (@set 1 2 3)) :@set
  "@set constructor produces :@set type")

(assert-eq (string (set 1 2 3)) "|1 2 3|"
  "set displays as |1 2 3|")

(assert-eq (string (@set 1 2 3)) "@|1 2 3|"
  "mutable set displays as @|1 2 3|")

(assert-eq (string (set)) "||"
  "empty set displays as ||")

(assert-eq (string (@set)) "@||"
  "empty mutable set displays as @||")

# ============================================================================
# Deduplication
# ============================================================================

(assert-eq (length (set 1 1 2)) 2
  "set deduplicates: (set 1 1 2) has 2 elements")

(assert-eq (length (set 1 2 3 1 2 3)) 3
  "set deduplicates multiple duplicates")

(assert-eq (length (@set 1 1 2)) 2
  "@set deduplicates: (@set 1 1 2) has 2 elements")

(assert-eq (length (set)) 0
  "empty set has 0 elements")

# ============================================================================
# Type Checking
# ============================================================================

(assert-true (set? (set 1 2 3))
  "set? returns true for immutable set literal")

(assert-true (set? @|1 2 3|)
  "set? returns true for mutable set literal")

(assert-true (set? (set 1 2 3))
  "set? returns true for set constructor result")

(assert-true (set? (@set 1 2 3))
  "set? returns true for @set constructor result")

(assert-false (set? [1 2 3])
  "set? returns false for tuple")

(assert-false (set? @[1 2 3])
  "set? returns false for array")

(assert-false (set? (list 1 2 3))
  "set? returns false for list")

(assert-false (set? "hello")
  "set? returns false for string")

(assert-false (set? {:a 1})
  "set? returns false for struct")

(assert-eq (type-of (set 1 2 3)) :set
  "type-of returns :set for immutable set")

(assert-eq (type-of @|1 2 3|) :@set
  "type-of returns :@set for mutable set")

(assert-eq (type-of (set 1 2 3)) :set
  "type-of returns :set for set constructor result")

(assert-eq (type-of (@set 1 2 3)) :@set
  "type-of returns :@set for @set constructor result")

# ============================================================================
# Equality
# ============================================================================

(assert-eq (set 1 2 3) (set 1 2 3)
  "identical immutable sets are equal")

(assert-eq (set 1 2 3) (set 3 2 1)
  "immutable sets are equal regardless of order")

(assert-eq (set 1 2 3) (set 2 1 3)
  "immutable sets are equal regardless of order (different permutation)")

(assert-false (= (set 1 2 3) (set 1 2))
  "immutable sets with different elements are not equal")

(assert-false (= (set 1 2 3) (set 1 2 3 4))
  "immutable sets with different sizes are not equal")

(assert-eq @|1 2 3| @|1 2 3|
  "identical mutable sets are equal")

(assert-eq @|1 2 3| @|3 2 1|
  "mutable sets are equal regardless of order")

(assert-false (= @|1 2 3| @|1 2|)
  "mutable sets with different elements are not equal")

(assert-false (= (set 1 2 3) @|1 2 3|)
  "immutable and mutable sets are not equal (different types)")

(assert-eq (set 1 2 3) (set 1 2 3)
  "sets created with constructor are equal")

(assert-eq (set 1 2 3) (set 3 2 1)
  "sets created with constructor are equal regardless of order")

(assert-false (= (set 1 2 3) (@set 1 2 3))
  "immutable and mutable sets from constructors are not equal")

# ============================================================================
# Freezing on Insert
# ============================================================================

(assert-eq (type-of (get (set->array (set @[1 2])) 0)) :tuple
  "mutable array is frozen when inserted into set")

(assert-eq (type-of (get (set->array (set @{:a 1})) 0)) :struct
  "mutable table is frozen when inserted into set")

(assert-eq (type-of (get (set->array (set @"hello")) 0)) :string
  "mutable buffer is frozen when inserted into set")

(assert-false (array? (get (set->array (set @[1 2])) 0))
  "frozen array is not an array")

(assert-false (table? (get (set->array (set @{:a 1})) 0))
  "frozen table is not a table")

# ============================================================================
# Length and Empty
# ============================================================================

(assert-eq (length (set 1 2 3)) 3
  "length of set with 3 elements is 3")

(assert-eq (length (set 1 2 3)) 3
  "length of set from constructor is 3")

(assert-eq (length ||) 0
  "length of empty set is 0")

(assert-eq (length (set)) 0
  "length of empty set from constructor is 0")

(assert-true (empty? ||)
  "empty? returns true for empty immutable set")

(assert-true (empty? (set))
  "empty? returns true for empty set from constructor")

(assert-false (empty? (set 1))
  "empty? returns false for non-empty immutable set")

(assert-false (empty? (set 1))
  "empty? returns false for non-empty set from constructor")

(assert-true (empty? @||)
  "empty? returns true for empty mutable set")

(assert-true (empty? (@set))
  "empty? returns true for empty mutable set from constructor")

(assert-false (empty? @|1|)
  "empty? returns false for non-empty mutable set")

# ============================================================================
# Membership (contains?)
# ============================================================================

(assert-true (contains? (set 1 2 3) 2)
  "contains? returns true for element in set")

(assert-true (contains? (set 1 2 3) 1)
  "contains? returns true for first element")

(assert-true (contains? (set 1 2 3) 3)
  "contains? returns true for last element")

(assert-false (contains? (set 1 2 3) 4)
  "contains? returns false for element not in set")

(assert-false (contains? (set 1 2 3) 0)
  "contains? returns false for element not in set (before range)")

(assert-true (contains? @|1 2 3| 2)
  "contains? returns true for element in mutable set")

(assert-false (contains? @|1 2 3| 4)
  "contains? returns false for element not in mutable set")

(assert-false (contains? || 1)
  "contains? returns false for element in empty set")

(assert-false (contains? (set) 1)
  "contains? returns false for element in empty set from constructor")

# ============================================================================
# Conversions: set->array
# ============================================================================

(assert-true (tuple? (set->array (set 1 2 3)))
  "set->array on immutable set returns a tuple")

(assert-eq (length (set->array (set 1 2 3))) 3
  "set->array preserves element count")

(assert-eq (length (set->array ||)) 0
  "set->array of empty set returns empty tuple")

(assert-true (tuple? (set->array (set 1 2 3)))
  "set->array works with constructor-created sets")

(assert-true (array? (set->array @|1 2 3|))
  "set->array on mutable set returns an array")

# ============================================================================
# Conversions: seq->set
# ============================================================================

(assert-eq (seq->set (list 1 2 3)) (set 1 2 3)
  "seq->set from list creates immutable set")

(assert-eq (seq->set (list 1 1 2)) (set 1 2)
  "seq->set deduplicates elements")

(assert-eq (seq->set (list)) ||
  "seq->set of empty list creates empty set")

(assert-true (set? (seq->set (list 1 2 3)))
  "seq->set from list result is a set")

(assert-eq (type-of (seq->set (list 1 2 3))) :set
  "seq->set from list creates immutable set")

(assert-eq (seq->set [1 2 3]) (set 1 2 3)
  "seq->set from tuple creates immutable set")

(assert-eq (type-of (seq->set @[1 2 3])) :@set
  "seq->set from array creates mutable set")

(assert-eq (seq->set "abc") (set "a" "b" "c")
  "seq->set from string creates immutable set of chars")

(assert-eq (type-of (seq->set "abc")) :set
  "seq->set from string creates immutable set")

(assert-eq (type-of (seq->set @"abc")) :@set
  "seq->set from buffer creates mutable set")

# ============================================================================
# Freeze/Thaw
# ============================================================================

(assert-eq (freeze @|1 2 3|) (set 1 2 3)
  "freeze converts mutable set to immutable")

(assert-eq (type-of (freeze @|1 2 3|)) :set
  "freeze produces :set type")

(assert-eq (thaw (set 1 2 3)) @|1 2 3|
  "thaw converts immutable set to mutable")

(assert-eq (type-of (thaw (set 1 2 3))) :@set
  "thaw produces :@set type")

(assert-eq (freeze (freeze @|1 2 3|)) (set 1 2 3)
  "freeze is idempotent on already-frozen sets")

(assert-eq (thaw (thaw (set 1 2 3))) @|1 2 3|
  "thaw is idempotent on already-thawed sets")

(assert-eq (freeze (thaw (set 1 2 3))) (set 1 2 3)
  "freeze after thaw returns to original")

(assert-eq (thaw (freeze @|1 2 3|)) @|1 2 3|
  "thaw after freeze returns to original")

# ============================================================================
# Set with Various Element Types
# ============================================================================

(assert-eq (length (set 1 "hello" :keyword)) 3
  "set can contain mixed types")

(assert-true (contains? (set 1 "hello" :keyword) "hello")
  "set contains string element")

(assert-true (contains? (set 1 "hello" :keyword) :keyword)
  "set contains keyword element")

(assert-eq (length (set true false nil)) 3
  "set can contain booleans and nil")

(assert-true (contains? (set true false nil) nil)
  "set contains nil")

(assert-true (contains? (set true false nil) true)
  "set contains true")

# ============================================================================
# Set with Nested Structures
# ============================================================================

(assert-eq (length (set [1 2] [3 4])) 2
  "set can contain tuples")

(assert-true (contains? (set [1 2] [3 4]) [1 2])
  "set contains tuple element")

(assert-eq (length (set {:a 1} {:b 2})) 2
  "set can contain structs")

(assert-true (contains? (set {:a 1} {:b 2}) {:a 1})
  "set contains struct element")

# ============================================================================
# Set Operations Preserve Immutability
# ============================================================================

(def original-set (set 1 2 3))
(def converted-arr (set->array original-set))
(assert-eq original-set (set 1 2 3)
  "set->array does not modify original set")

(def original-list (list 1 2 3))
(def converted-set (seq->set original-list))
(assert-eq original-list (list 1 2 3)
  "seq->set does not modify original list")

# ============================================================================
# Element Operations
# ============================================================================

# add on immutable set returns new set
(assert-eq (add (set 1 2) 3) (set 1 2 3)
  "add to immutable set returns new set")

(assert-eq (add (set 1 2) 2) (set 1 2)
  "add existing element is no-op on immutable set")

# add on mutable set mutates
(def ms @|1 2|)
(add ms 3)
(assert-true (contains? ms 3)
  "add mutates mutable set")

# del on immutable set returns new set
(assert-eq (del (set 1 2 3) 2) (set 1 3)
  "del from immutable set returns new set")

(assert-eq (del (set 1 2 3) 4) (set 1 2 3)
  "del non-existent element is no-op on immutable set")

# del on mutable set mutates
(def ms2 @|1 2 3|)
(del ms2 2)
(assert-false (contains? ms2 2)
  "del mutates mutable set")

# ============================================================================
# Set Algebra
# ============================================================================

(assert-eq (union (set 1 2) (set 2 3)) (set 1 2 3)
  "union of immutable sets")

(assert-eq (intersection (set 1 2 3) (set 2 3 4)) (set 2 3)
  "intersection of immutable sets")

(assert-eq (difference (set 1 2 3) (set 2 3)) (set 1)
  "difference of immutable sets")

# mutable set algebra
(assert-eq (union @|1 2| @|2 3|) @|1 2 3|
  "union of mutable sets")

(assert-eq (intersection @|1 2 3| @|2 3 4|) @|2 3|
  "intersection of mutable sets")

(assert-eq (difference @|1 2 3| @|2 3|) @|1|
  "difference of mutable sets")

# ============================================================================
# Match Type Guards
# ============================================================================

(assert-eq (match (set 1 2 3)
             (|s| (length s))
             (_ :no-match))
           3
           "match immutable set")

(assert-eq (match @|1 2|
             (@|s| (length s))
             (_ :no-match))
           2
           "match mutable set")

(assert-eq (match (set 1 2 3)
             (@|s| :mutable)
             (|s| :immutable)
             (_ :no-match))
           :immutable
           "match distinguishes set types")

# ============================================================================
# Each Iteration
# ============================================================================

(var sum 0)
(each x (set 1 2 3)
  (assign sum (+ sum x)))
(assert-eq sum 6
  "each iterates over set elements")

# ============================================================================
# Map
# ============================================================================

(def doubled (map (fn (x) (* x 2)) (set 1 2 3)))
(assert-true (set? doubled)
  "map on set returns a set")

(assert-true (contains? doubled 2)
  "map result contains mapped element 2")

(assert-true (contains? doubled 4)
  "map result contains mapped element 4")

(assert-true (contains? doubled 6)
  "map result contains mapped element 6")

# ============================================================================
# Display
# ============================================================================

(assert-eq (string/format "{}" ||) "||"
  "empty immutable set displays as ||")

(assert-eq (string/format "{}" @||) "@||"
  "empty mutable set displays as @||")
