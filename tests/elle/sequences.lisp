(elle/epoch 8)
## Sequence Operation Tests
##
## Migrated from tests/property/sequences.rs
## Type preservation and involution properties hold for all values.


## reverse_involution_list
## Verify that reversing a list twice returns the original list

(assert (= (reverse (reverse (list 1 2 3))) (list 1 2 3)) "reverse involution: (list 1 2 3)")

(assert (= (reverse (reverse (list -5 0 7))) (list -5 0 7)) "reverse involution: (list -5 0 7)")

(assert (= (reverse (reverse (list))) (list)) "reverse involution: empty list")

## reverse_involution_array
## Verify that reversing an array twice returns the original array

(assert (= (reverse (reverse [1 2 3])) [1 2 3]) "reverse involution: [1 2 3]")

(assert (= (reverse (reverse [-5 0 7])) [-5 0 7]) "reverse involution: [-5 0 7]")

## rest_preserves_list_type
## Verify that rest of a list is a list

(assert (list? (rest (list 1 2))) "rest preserves list type: (list 1 2)")

(assert (list? (rest (list 1 2 3))) "rest preserves list type: (list 1 2 3)")

## rest_preserves_array_type
## Verify that rest of an array is an array

(assert (array? (rest [1 2])) "rest preserves array type: [1 2]")

(assert (array? (rest [1 2 3])) "rest preserves array type: [1 2 3]")

## rest_preserves_array_type
## Verify that rest of an array is an array

(assert (array? (rest @[1 2])) "rest preserves array type: @[1 2]")

(assert (array? (rest @[1 2 3])) "rest preserves array type: @[1 2 3]")

## rest_preserves_string_type
## Verify that rest of a string is a string

(assert (string? (rest "hello")) "rest preserves string type: \"hello\"")

(assert (string? (rest "ab")) "rest preserves string type: \"ab\"")

## reverse_preserves_array_type
## Verify that reverse of an array is an array

(assert (array? (reverse @[1 2])) "reverse preserves array type: @[1 2]")

(assert (array? (reverse @[1 2 3])) "reverse preserves array type: @[1 2 3]")
