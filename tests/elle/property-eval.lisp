(elle/epoch 8)

# eval property tests
# Migrated from tests/property/eval.rs
# eval's behavior doesn't vary with integer values — representative examples suffice.

# eval_quoted_integer_is_identity
# eval with quoted integers returns the integer unchanged
(assert (= (eval '42) 42) "eval quoted 42")
(assert (= (eval '0) 0) "eval quoted 0")
(assert (= (eval '-99) -99) "eval quoted -99")

# eval_quoted_addition
# eval with quoted addition expressions computes the sum
(assert (= (eval '(+ 3 5)) 8) "eval quoted addition 3+5")
(assert (= (eval '(+ -10 7)) -3) "eval quoted addition -10+7")
(assert (= (eval '(+ 0 0)) 0) "eval quoted addition 0+0")

# eval_quoted_multiplication
# eval with quoted multiplication expressions computes the product
(assert (= (eval '(* 6 7)) 42) "eval quoted multiplication 6*7")
(assert (= (eval '(* -3 4)) -12) "eval quoted multiplication -3*4")
(assert (= (eval '(* 0 100)) 0) "eval quoted multiplication 0*100")

# eval_env_binding_addition (REMOVED)
# Environment argument support was intentionally removed from eval.
# Tests that relied on (eval expr env) have been removed.

# eval_list_construction_matches_quoted
# eval with dynamically constructed list matches eval with quoted list
(assert (= (eval '(+ 3 5)) 8) "eval quoted list construction")
(assert (= (eval (list '+ 3 5)) 8) "eval dynamic list construction")

# eval_result_in_addition
# eval result can be used in arithmetic expressions
(assert (= (+ 10 (eval '32)) 42) "eval result in addition")

# eval_quoted_string_is_identity
# eval with quoted strings returns the string unchanged
(assert (= (eval '"hello") "hello") "eval quoted string hello")
(assert (= (eval '"") "") "eval quoted empty string")

# eval_quoted_boolean
# eval with quoted booleans returns the boolean unchanged
(assert (= (eval 'true) true) "eval quoted true")
(assert (= (eval 'false) false) "eval quoted false")
