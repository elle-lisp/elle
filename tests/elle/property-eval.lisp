(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# eval property tests
# Migrated from tests/property/eval.rs
# eval's behavior doesn't vary with integer values — representative examples suffice.

# eval_quoted_integer_is_identity
# eval with quoted integers returns the integer unchanged
(assert-eq (eval '42) 42 "eval quoted 42")
(assert-eq (eval '0) 0 "eval quoted 0")
(assert-eq (eval '-99) -99 "eval quoted -99")

# eval_quoted_addition
# eval with quoted addition expressions computes the sum
(assert-eq (eval '(+ 3 5)) 8 "eval quoted addition 3+5")
(assert-eq (eval '(+ -10 7)) -3 "eval quoted addition -10+7")
(assert-eq (eval '(+ 0 0)) 0 "eval quoted addition 0+0")

# eval_quoted_multiplication
# eval with quoted multiplication expressions computes the product
(assert-eq (eval '(* 6 7)) 42 "eval quoted multiplication 6*7")
(assert-eq (eval '(* -3 4)) -12 "eval quoted multiplication -3*4")
(assert-eq (eval '(* 0 100)) 0 "eval quoted multiplication 0*100")

# eval_env_binding_addition
# eval with environment bindings resolves variables correctly
(assert-eq (eval '(+ x y) {:x 10 :y 20}) 30 "eval env binding 10+20")
(assert-eq (eval '(+ x y) {:x -5 :y 5}) 0 "eval env binding -5+5")

# eval_list_construction_matches_quoted
# eval with dynamically constructed list matches eval with quoted list
(assert-eq (eval '(+ 3 5)) 8 "eval quoted list construction")
(assert-eq (eval (list '+ 3 5)) 8 "eval dynamic list construction")

# eval_result_in_addition
# eval result can be used in arithmetic expressions
(assert-eq (+ 10 (eval '32)) 42 "eval result in addition")

# eval_quoted_string_is_identity
# eval with quoted strings returns the string unchanged
(assert-eq (eval '"hello") "hello" "eval quoted string hello")
(assert-eq (eval '"") "" "eval quoted empty string")

# eval_quoted_boolean
# eval with quoted booleans returns the boolean unchanged
(assert-eq (eval 'true) true "eval quoted true")
(assert-eq (eval 'false) false "eval quoted false")
