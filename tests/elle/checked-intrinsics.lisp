(elle/epoch 9)
# --checked-intrinsics tests
#
# These tests verify that --checked-intrinsics catches type errors
# that would produce garbage in default mode.
#
# This file runs under --checked-intrinsics (set by smoke-vm).
# It tests the happy path — correct types work fine.

# test_checked_add
(assert (= (%add 1 2) 3) "checked %add int")
(assert (= (%add 1.5 2.5) 4.0) "checked %add float")

# test_checked_sub
(assert (= (%sub 10 3) 7) "checked %sub binary")
(assert (= (%sub 5) -5) "checked %sub unary")

# test_checked_mul
(assert (= (%mul 4 5) 20) "checked %mul int")

# test_checked_div
(assert (= (%div 20 4) 5) "checked %div exact")

# test_checked_comparisons
(assert (%lt 1 2) "checked %lt")
(assert (%gt 2 1) "checked %gt")
(assert (%le 1 1) "checked %le")
(assert (%ge 2 1) "checked %ge")
(assert (%eq 1 1) "checked %eq")
(assert (%ne 1 2) "checked %ne")

# test_checked_type_predicates
(assert (%int? 42) "checked %int?")
(assert (%float? 3.14) "checked %float?")
(assert (%string? "hi") "checked %string?")
(assert (%nil? nil) "checked %nil?")

# test_checked_data_access
(assert (= (%length [1 2 3]) 3) "checked %length")
(assert (= (%get [10 20 30] 1) 20) "checked %get")

# test_intrinsic_as_callable_value
# Under --checked-intrinsics, %add is a real NativeFn that can be
# stored and passed to higher-order functions.
(def my-add %add)
(assert (= (my-add 10 20) 30) "checked: %add as callable value")

# test_intrinsic_in_map
(assert (= (map (fn [x] (%mul x x)) '(1 2 3)) '(1 4 9))
        "checked: intrinsic in map")

(println "all checked-intrinsic tests passed")
