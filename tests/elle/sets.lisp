(elle/epoch 9)
## Set types test suite
##
## Comprehensive tests for set types (immutable and mutable).
## Covers syntax, value creation, operations, and integration with other language features.
## Issue #509: Set type implementation.


# ============================================================================
# Set Literal Syntax — Immutable Sets
# ============================================================================

(assert (= (type (set 1 2 3)) :set) "set literal has type :set")

(assert (= (length (set 1 2 3)) 3) "set literal has 3 elements")

(assert (= (length ||) 0) "empty set literal")

(assert (= (set 1 2 3) (set 1 2 3)) "set literals are equal")

(assert (= (set 3 1 2) (set 1 2 3)) "set order doesn't matter (both are equal)")

(assert (= (length (set 1 1 2)) 2) "set deduplicates elements")

# ============================================================================
# Set Literal Syntax — Mutable Sets
# ============================================================================

(assert (= (type @|1 2 3|) :@set) "mutable set literal has type :@set")

(assert (= (length @|1 2 3|) 3) "mutable set literal has 3 elements")

(assert (= (length @||) 0) "empty mutable set literal")

(assert (= @|1 2 3| @|1 2 3|) "mutable set literals are equal")

(assert (= @|3 1 2| @|1 2 3|)
        "mutable set order doesn't matter (both are equal)")

(assert (= (length @|1 1 2|) 2) "mutable set deduplicates elements")

# ============================================================================
# Set Predicate Covers Both Types
# ============================================================================

(assert (set? (set 1 2 3)) "set? true for immutable set")

(assert (set? @|1 2 3|) "set? true for mutable set")

(assert (not (set? [1 2 3])) "set? false for array")

(assert (not (set? (list 1 2 3))) "set? false for list")

(assert (not (set? "hello")) "set? false for string")

# ============================================================================
# :@keyword Syntax
# ============================================================================

(assert (= :@set :@set) ":@set is a valid keyword")

(assert (keyword? :@set) ":@set is a keyword")

# ============================================================================
# Or-Patterns
# ============================================================================

(assert (= (match 1
             (or 1 3 5) :odd
             _ :even)
           :odd)
        "or-pattern: 1 is odd")

(assert (= (match 2
             (or 1 3 5) :odd
             _ :even)
           :even)
        "or-pattern: 2 is even")

(assert (= (match 3
             (or 1 3 5) :odd
             _ :even)
           :odd)
        "or-pattern: 3 is odd")

(assert (= (match 5
             (or 1 3 5) :odd
             _ :even)
           :odd)
        "or-pattern: 5 is odd")

(assert (= (match 4
             (or 1 3 5) :odd
             _ :even)
           :even)
        "or-pattern: 4 is even")

# ============================================================================
# Set Inside a List (Set as Expression via Constructor)
# ============================================================================

(def s (set 1 2 3))
(assert (set? s) "set assigned to variable")

(assert (= (length s) 3) "set variable has correct length")

# ============================================================================
# Nested Sets (via Constructor)
# ============================================================================

(assert (= (length (set (set 1 2))) 1) "set containing a set")

(assert (set? (get (set->array (set (set 1 2))) 0)) "nested set is a set")

# ============================================================================
# Mixed Immutable and Mutable Sets
# ============================================================================

(assert (= (set 1 2 3) @|1 2 3|) "set = @set (cross-mutability equality)")

(assert (set? (set 1 2 3)) "immutable set passes set? predicate")

(assert (set? @|1 2 3|) "mutable set passes set? predicate")

# ============================================================================
# Set Literals in Various Contexts
# ============================================================================

(assert (= (+ 1 (length (set 1 2 3))) 4) "set literal in call position")

(def my-set (set 10 20 30))
(assert (= (length my-set) 3) "set literal assigned to variable")

(assert (= (length (list (set 1) (set 2) (set 3))) 3) "set literals in list")

# ============================================================================
# Empty Sets
# ============================================================================

(assert (= || ||) "empty immutable sets are equal")

(assert (= @|| @||) "empty mutable sets are equal")

(assert (= (length ||) 0) "empty immutable set has length 0")

(assert (= (length @||) 0) "empty mutable set has length 0")

(assert (empty? ||) "empty immutable set is empty?")

(assert (empty? @||) "empty mutable set is empty?")

# ============================================================================
# Set Construction and Display
# ============================================================================

(assert (set? (set 1 2 3)) "set constructor creates a set")

(assert (set? (@set 1 2 3)) "@set constructor creates a mutable set")

(assert (= (type (set 1 2 3)) :set) "set constructor produces :set type")

(assert (= (type (@set 1 2 3)) :@set) "@set constructor produces :@set type")

(assert (= (string (set 1 2 3)) "|1 2 3|") "set displays as |1 2 3|")

(assert (= (string (@set 1 2 3)) "@|1 2 3|") "mutable set displays as @|1 2 3|")

(assert (= (string (set)) "||") "empty set displays as ||")

(assert (= (string (@set)) "@||") "empty mutable set displays as @||")

# ============================================================================
# Deduplication
# ============================================================================

(assert (= (length (set 1 1 2)) 2)
        "set deduplicates: (set 1 1 2) has 2 elements")

(assert (= (length (set 1 2 3 1 2 3)) 3) "set deduplicates multiple duplicates")

(assert (= (length (@set 1 1 2)) 2)
        "@set deduplicates: (@set 1 1 2) has 2 elements")

(assert (= (length (set)) 0) "empty set has 0 elements")

# ============================================================================
# Type Checking
# ============================================================================

(assert (set? (set 1 2 3)) "set? returns true for immutable set literal")

(assert (set? @|1 2 3|) "set? returns true for mutable set literal")

(assert (set? (set 1 2 3)) "set? returns true for set constructor result")

(assert (set? (@set 1 2 3)) "set? returns true for @set constructor result")

(assert (not (set? [1 2 3])) "set? returns false for array")

(assert (not (set? @[1 2 3])) "set? returns false for array")

(assert (not (set? (list 1 2 3))) "set? returns false for list")

(assert (not (set? "hello")) "set? returns false for string")

(assert (not (set? {:a 1})) "set? returns false for struct")

(assert (= (type-of (set 1 2 3)) :set) "type-of returns :set for immutable set")

(assert (= (type-of @|1 2 3|) :@set) "type-of returns :@set for mutable set")

(assert (= (type-of (set 1 2 3)) :set)
        "type-of returns :set for set constructor result")

(assert (= (type-of (@set 1 2 3)) :@set)
        "type-of returns :@set for @set constructor result")

# ============================================================================
# Equality
# ============================================================================

(assert (= (set 1 2 3) (set 1 2 3)) "identical immutable sets are equal")

(assert (= (set 1 2 3) (set 3 2 1))
        "immutable sets are equal regardless of order")

(assert (= (set 1 2 3) (set 2 1 3))
        "immutable sets are equal regardless of order (different permutation)")

(assert (not (= (set 1 2 3) (set 1 2)))
        "immutable sets with different elements are not equal")

(assert (not (= (set 1 2 3) (set 1 2 3 4)))
        "immutable sets with different sizes are not equal")

(assert (= @|1 2 3| @|1 2 3|) "identical mutable sets are equal")

(assert (= @|1 2 3| @|3 2 1|) "mutable sets are equal regardless of order")

(assert (not (= @|1 2 3| @|1 2|))
        "mutable sets with different elements are not equal")

(assert (= (set 1 2 3) @|1 2 3|)
        "set = @set (cross-mutability equality, different types)")

(assert (= (set 1 2 3) (set 1 2 3)) "sets created with constructor are equal")

(assert (= (set 1 2 3) (set 3 2 1))
        "sets created with constructor are equal regardless of order")

(assert (= (set 1 2 3) (@set 1 2 3))
        "set = @set from constructors (cross-mutability equality)")

# ============================================================================
# Freezing on Insert
# ============================================================================

(assert (= (type-of (get (set->array (set @[1 2])) 0)) :array)
        "mutable @struct is frozen when inserted into set")

(assert (= (type-of (get (set->array (set @{:a 1})) 0)) :struct)
        "mutable @string is frozen when inserted into set")

(assert (array? (get (set->array (set @[1 2])) 0)) "frozen array is an array")

(assert (struct? (get (set->array (set @{:a 1})) 0))
        "frozen @struct is a struct")

# ============================================================================
# Length and Empty
# ============================================================================

(assert (= (length (set 1 2 3)) 3) "length of set with 3 elements is 3")

(assert (= (length (set 1 2 3)) 3) "length of set from constructor is 3")

(assert (= (length ||) 0) "length of empty set is 0")

(assert (= (length (set)) 0) "length of empty set from constructor is 0")

(assert (empty? ||) "empty? returns true for empty immutable set")

(assert (empty? (set)) "empty? returns true for empty set from constructor")

(assert (not (empty? (set 1)))
        "empty? returns false for non-empty immutable set")

(assert (not (empty? (set 1)))
        "empty? returns false for non-empty set from constructor")

(assert (empty? @||) "empty? returns true for empty mutable set")

(assert (empty? (@set))
        "empty? returns true for empty mutable set from constructor")

(assert (not (empty? @|1|)) "empty? returns false for non-empty mutable set")

# ============================================================================
# Membership (contains?)
# ============================================================================

(assert (contains? (set 1 2 3) 2) "contains? returns true for element in set")

(assert (contains? (set 1 2 3) 1) "contains? returns true for first element")

(assert (contains? (set 1 2 3) 3) "contains? returns true for last element")

(assert (not (contains? (set 1 2 3) 4))
        "contains? returns false for element not in set")

(assert (not (contains? (set 1 2 3) 0))
        "contains? returns false for element not in set (before range)")

(assert (contains? @|1 2 3| 2)
        "contains? returns true for element in mutable set")

(assert (not (contains? @|1 2 3| 4))
        "contains? returns false for element not in mutable set")

(assert (not (contains? || 1))
        "contains? returns false for element in empty set")

(assert (not (contains? (set) 1))
        "contains? returns false for element in empty set from constructor")

# ============================================================================
# Conversions: set->array
# ============================================================================

(assert (array? (set->array (set 1 2 3)))
        "set->array on immutable set returns an array")

(assert (= (length (set->array (set 1 2 3))) 3)
        "set->array preserves element count")

(assert (= (length (set->array ||)) 0)
        "set->array of empty set returns empty array")

(assert (array? (set->array (set 1 2 3)))
        "set->array works with constructor-created sets")

(assert (array? (set->array @|1 2 3|))
        "set->array on mutable set returns an array")

# ============================================================================
# Conversions: seq->set
# ============================================================================

(assert (= (seq->set (list 1 2 3)) (set 1 2 3))
        "seq->set from list creates immutable set")

(assert (= (seq->set (list 1 1 2)) (set 1 2)) "seq->set deduplicates elements")

(assert (= (seq->set (list)) ||) "seq->set of empty list creates empty set")

(assert (set? (seq->set (list 1 2 3))) "seq->set from list result is a set")

(assert (= (type-of (seq->set (list 1 2 3))) :set)
        "seq->set from list creates immutable set")

(assert (= (seq->set [1 2 3]) (set 1 2 3))
        "seq->set from array creates immutable set")

(assert (= (type-of (seq->set @[1 2 3])) :@set)
        "seq->set from array creates mutable set")

(assert (= (seq->set "abc") (set "a" "b" "c"))
        "seq->set from string creates immutable set of chars")

(assert (= (type-of (seq->set "abc")) :set)
        "seq->set from string creates immutable set")

(assert (= (type-of (seq->set (thaw "abc"))) :@set)
        "seq->set from @string creates mutable set")

# ============================================================================
# Freeze/Thaw
# ============================================================================

(assert (= (freeze @|1 2 3|) (set 1 2 3))
        "freeze converts mutable set to immutable")

(assert (= (type-of (freeze @|1 2 3|)) :set) "freeze produces :set type")

(assert (= (thaw (set 1 2 3)) @|1 2 3|) "thaw converts immutable set to mutable")

(assert (= (type-of (thaw (set 1 2 3))) :@set) "thaw produces :@set type")

(assert (= (freeze (freeze @|1 2 3|)) (set 1 2 3))
        "freeze is idempotent on already-frozen sets")

(assert (= (thaw (thaw (set 1 2 3))) @|1 2 3|)
        "thaw is idempotent on already-thawed sets")

(assert (= (freeze (thaw (set 1 2 3))) (set 1 2 3))
        "freeze after thaw returns to original")

(assert (= (thaw (freeze @|1 2 3|)) @|1 2 3|)
        "thaw after freeze returns to original")

# ============================================================================
# Set with Various Element Types
# ============================================================================

(assert (= (length (set 1 "hello" :keyword)) 3) "set can contain mixed types")

(assert (contains? (set 1 "hello" :keyword) "hello")
        "set contains string element")

(assert (contains? (set 1 "hello" :keyword) :keyword)
        "set contains keyword element")

(assert (= (length (set true false nil)) 3) "set can contain booleans and nil")

(assert (contains? (set true false nil) nil) "set contains nil")

(assert (contains? (set true false nil) true) "set contains true")

# ============================================================================
# Set with Nested Structures
# ============================================================================

(assert (= (length (set [1 2] [3 4])) 2) "set can contain arrays")

(assert (contains? (set [1 2] [3 4]) [1 2]) "set contains array element")

(assert (= (length (set {:a 1} {:b 2})) 2) "set can contain structs")

(assert (contains? (set {:a 1} {:b 2}) {:a 1}) "set contains struct element")

# ============================================================================
# Set Operations Preserve Immutability
# ============================================================================

(def original-set (set 1 2 3))
(def converted-arr (set->array original-set))
(assert (= original-set (set 1 2 3)) "set->array does not modify original set")

(def original-list (list 1 2 3))
(def converted-set (seq->set original-list))
(assert (= original-list (list 1 2 3)) "seq->set does not modify original list")

# ============================================================================
# Element Operations
# ============================================================================

# add on immutable set returns new set
(assert (= (add (set 1 2) 3) (set 1 2 3)) "add to immutable set returns new set")

(assert (= (add (set 1 2) 2) (set 1 2))
        "add existing element is no-op on immutable set")

# add on mutable set mutates
(def ms @|1 2|)
(add ms 3)
(assert (contains? ms 3) "add mutates mutable set")

# del on immutable set returns new set
(assert (= (del (set 1 2 3) 2) (set 1 3))
        "del from immutable set returns new set")

(assert (= (del (set 1 2 3) 4) (set 1 2 3))
        "del non-existent element is no-op on immutable set")

# del on mutable set mutates
(def ms2 @|1 2 3|)
(del ms2 2)
(assert (not (contains? ms2 2)) "del mutates mutable set")

# ============================================================================
# Set Algebra
# ============================================================================

(assert (= (union (set 1 2) (set 2 3)) (set 1 2 3)) "union of immutable sets")

(assert (= (intersection (set 1 2 3) (set 2 3 4)) (set 2 3))
        "intersection of immutable sets")

(assert (= (difference (set 1 2 3) (set 2 3)) (set 1))
        "difference of immutable sets")

# mutable set algebra
(assert (= (union @|1 2| @|2 3|) @|1 2 3|) "union of mutable sets")

(assert (= (intersection @|1 2 3| @|2 3 4|) @|2 3|)
        "intersection of mutable sets")

(assert (= (difference @|1 2 3| @|2 3|) @|1|) "difference of mutable sets")

# ============================================================================
# Match Type Guards
# ============================================================================

(assert (= (match (set 1 2 3)
             |s| (length s)
             _ :no-match)
           3)
        "match immutable set")

(assert (= (match @|1 2|
             @|s| (length s)
             _ :no-match)
           2)
        "match mutable set")

(assert (= (match (set 1 2 3)
             @|s| :mutable
             |s| :immutable
             _ :no-match)
           :immutable)
        "match distinguishes set types")

# ============================================================================
# Each Iteration
# ============================================================================

(def @sum 0)
(each x (set 1 2 3)
  (assign sum (+ sum x)))
(assert (= sum 6) "each iterates over set elements")

# ============================================================================
# Map
# ============================================================================

(def doubled (map (fn (x) (* x 2)) (set 1 2 3)))
(assert (set? doubled) "map on set returns a set")

(assert (contains? doubled 2) "map result contains mapped element 2")

(assert (contains? doubled 4) "map result contains mapped element 4")

(assert (contains? doubled 6) "map result contains mapped element 6")

# ============================================================================
# Display
# ============================================================================

(assert (= (string/format "{}" ||) "||") "empty immutable set displays as ||")

(assert (= (string/format "{}" @||) "@||") "empty mutable set displays as @||")
