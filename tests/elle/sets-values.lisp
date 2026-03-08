## Set Value Tests
##
## Tests set value creation, display, equality, and basic operations.
## Chunk 3 of issue #509: set value implementation.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Set construction and display
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
# Type checking
# ============================================================================

(assert-true (set? |1 2 3|)
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

(assert-eq (type-of |1 2 3|) :set
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

(assert-eq |1 2 3| |1 2 3|
  "identical immutable sets are equal")

(assert-eq |1 2 3| |3 2 1|
  "immutable sets are equal regardless of order")

(assert-eq |1 2 3| |2 1 3|
  "immutable sets are equal regardless of order (different permutation)")

(assert-false (= |1 2 3| |1 2|)
  "immutable sets with different elements are not equal")

(assert-false (= |1 2 3| |1 2 3 4|)
  "immutable sets with different sizes are not equal")

(assert-eq @|1 2 3| @|1 2 3|
  "identical mutable sets are equal")

(assert-eq @|1 2 3| @|3 2 1|
  "mutable sets are equal regardless of order")

(assert-false (= @|1 2 3| @|1 2|)
  "mutable sets with different elements are not equal")

(assert-false (= |1 2 3| @|1 2 3|)
  "immutable and mutable sets are not equal (different types)")

(assert-eq (set 1 2 3) (set 1 2 3)
  "sets created with constructor are equal")

(assert-eq (set 1 2 3) (set 3 2 1)
  "sets created with constructor are equal regardless of order")

(assert-false (= (set 1 2 3) (@set 1 2 3))
  "immutable and mutable sets from constructors are not equal")

# ============================================================================
# Freezing on insert
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
# Length and empty
# ============================================================================

(assert-eq (length |1 2 3|) 3
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

(assert-false (empty? |1|)
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

(assert-true (contains? |1 2 3| 2)
  "contains? returns true for element in set")

(assert-true (contains? |1 2 3| 1)
  "contains? returns true for first element")

(assert-true (contains? |1 2 3| 3)
  "contains? returns true for last element")

(assert-false (contains? |1 2 3| 4)
  "contains? returns false for element not in set")

(assert-false (contains? |1 2 3| 0)
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

(assert-true (tuple? (set->array |1 2 3|))
  "set->array on immutable set returns a tuple")

(assert-eq (length (set->array |1 2 3|)) 3
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

(assert-eq (seq->set (list 1 2 3)) |1 2 3|
  "seq->set from list creates immutable set")

(assert-eq (seq->set (list 1 1 2)) |1 2|
  "seq->set deduplicates elements")

(assert-eq (seq->set (list)) ||
  "seq->set of empty list creates empty set")

(assert-true (set? (seq->set (list 1 2 3)))
  "seq->set from list result is a set")

(assert-eq (type-of (seq->set (list 1 2 3))) :set
  "seq->set from list creates immutable set")

(assert-eq (seq->set [1 2 3]) |1 2 3|
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
# Freeze/thaw
# ============================================================================

(assert-eq (freeze @|1 2 3|) |1 2 3|
  "freeze converts mutable set to immutable")

(assert-eq (type-of (freeze @|1 2 3|)) :set
  "freeze produces :set type")

(assert-eq (thaw |1 2 3|) @|1 2 3|
  "thaw converts immutable set to mutable")

(assert-eq (type-of (thaw |1 2 3|)) :@set
  "thaw produces :@set type")

(assert-eq (freeze (freeze @|1 2 3|)) |1 2 3|
  "freeze is idempotent on already-frozen sets")

(assert-eq (thaw (thaw |1 2 3|)) @|1 2 3|
  "thaw is idempotent on already-thawed sets")

(assert-eq (freeze (thaw |1 2 3|)) |1 2 3|
  "freeze after thaw returns to original")

(assert-eq (thaw (freeze @|1 2 3|)) @|1 2 3|
  "thaw after freeze returns to original")

# ============================================================================
# Set with various element types
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
# Set with nested structures
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
# Set operations preserve immutability
# ============================================================================

(def original-set |1 2 3|)
(def converted-arr (set->array original-set))
(assert-eq original-set |1 2 3|
  "set->array does not modify original set")

(def original-list (list 1 2 3))
(def converted-set (seq->set original-list))
(assert-eq original-list (list 1 2 3)
  "seq->set does not modify original list")
