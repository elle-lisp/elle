(elle/epoch 9)
# Regression: block + def + push returned broken heap ref
#
# The escape analysis in can_scope_allocate_block treated def bindings
# inside a block body as outer bindings (safe to return). RegionExit
# freed the mutable array while a reference to it was still live.
# Fix: result_is_safe tracks Define bindings in Begin nodes.

# Basic case: block + def + push + return
(def r1 (block (def a @[]) (push a 1) a))
(assert (= (type r1) :@array) "r1 should be @array")
(assert (= (length r1) 1) "r1 length should be 1")
(assert (= (get r1 0) 1) "r1[0] should be 1")

# Multiple pushes
(def r2 (block (def b @[]) (push b :x) (push b :y) (push b :z) b))
(assert (= (length r2) 3) "r2 length should be 3")
(assert (= (get r2 2) :z) "r2[2] should be :z")

# Mutable struct
(def r3 (block (def s @{}) (put s :k 42) s))
(assert (= (type r3) :@struct) "r3 should be @struct")
(assert (= (get r3 :k) 42) "r3:k should be 42")

# Immutable value in block is fine (should still scope-allocate)
(def r4 (block (def x 42) x))
(assert (= r4 42) "r4 should be 42")

# let* inside block (was already immune)
(def r5 (let* [a @[]] (push a 1) a))
(assert (= (type r5) :@array) "r5 should be @array")
(assert (= (get r5 0) 1) "r5[0] should be 1")

(println "bug-block-push: PASS")
