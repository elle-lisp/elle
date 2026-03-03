# Booleans — boolean literals, predicates, and match behavior

(import-file "./examples/assertions.lisp")

# Boolean literals
(assert-eq true true "true literal")
(assert-eq false false "false literal")

# Truthiness in conditionals
(assert-eq (if true 1 2) 1 "if true => then branch")
(assert-eq (if false 1 2) 2 "if false => else branch")

# Boolean predicate
(assert-true (boolean? true) "boolean? on true")
(assert-true (boolean? false) "boolean? on false")

# Match on boolean values
(var match-false (match false (true "yes") (false "no")))
(assert-eq match-false "no" "match false")
(var match-true (match true (true "yes") (false "no")))
(assert-eq match-true "yes" "match true")

# Quoted booleans
(assert-eq 'true true "quoted true is boolean")

# Read roundtrip
(assert-eq (read "true") true "read true")

# String conversion
(assert-eq (string true) "true" "string of true")

# Display roundtrip
(assert-eq (read (string true)) true "read(string(true)) roundtrip")
