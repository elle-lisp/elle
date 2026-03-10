## Sequence Operation Tests
##
## Migrated from tests/property/sequences.rs
## Type preservation and involution properties hold for all values.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## reverse_involution_list
## Verify that reversing a list twice returns the original list

(assert-eq (reverse (reverse (list 1 2 3))) (list 1 2 3)
  "reverse involution: (list 1 2 3)")

(assert-eq (reverse (reverse (list -5 0 7))) (list -5 0 7)
  "reverse involution: (list -5 0 7)")

(assert-eq (reverse (reverse (list))) (list)
  "reverse involution: empty list")

## reverse_involution_array
## Verify that reversing an array twice returns the original array

(assert-eq (reverse (reverse [1 2 3])) [1 2 3]
    "reverse involution: [1 2 3]")

(assert-eq (reverse (reverse [-5 0 7])) [-5 0 7]
   "reverse involution: [-5 0 7]")

## rest_preserves_list_type
## Verify that rest of a list is a list

(assert-true (list? (rest (list 1 2)))
  "rest preserves list type: (list 1 2)")

(assert-true (list? (rest (list 1 2 3)))
  "rest preserves list type: (list 1 2 3)")

## rest_preserves_array_type
## Verify that rest of an array is an array

(assert-true (array? (rest [1 2]))
    "rest preserves array type: [1 2]")

(assert-true (array? (rest [1 2 3]))
   "rest preserves array type: [1 2 3]")

## rest_preserves_array_type
## Verify that rest of an array is an array

(assert-true (array? (rest @[1 2]))
  "rest preserves array type: @[1 2]")

(assert-true (array? (rest @[1 2 3]))
  "rest preserves array type: @[1 2 3]")

## rest_preserves_string_type
## Verify that rest of a string is a string

(assert-true (string? (rest "hello"))
  "rest preserves string type: \"hello\"")

(assert-true (string? (rest "ab"))
  "rest preserves string type: \"ab\"")

## reverse_preserves_array_type
## Verify that reverse of an array is an array

(assert-true (array? (reverse @[1 2]))
  "reverse preserves array type: @[1 2]")

(assert-true (array? (reverse @[1 2 3]))
  "reverse preserves array type: @[1 2 3]")
