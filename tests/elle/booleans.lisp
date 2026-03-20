(elle/epoch 1)
# Booleans — boolean literals, predicates, and match behavior


# Boolean literals
(assert (= true true) "true literal")
(assert (= false false) "false literal")

# Truthiness in conditionals
(assert (= (if true 1 2) 1) "if true => then branch")
(assert (= (if false 1 2) 2) "if false => else branch")

# Boolean predicate
(assert (boolean? true) "boolean? on true")
(assert (boolean? false) "boolean? on false")

# Match on boolean values
(var match-false (match false (true "yes") (false "no")))
(assert (= match-false "no") "match false")
(var match-true (match true (true "yes") (false "no")))
(assert (= match-true "yes") "match true")

# Quoted booleans
(assert (= 'true true) "quoted true is boolean")

# Read roundtrip
(assert (= (read "true") true) "read true")

# String conversion
(assert (= (string true) "true") "string of true")

# Display roundtrip
(assert (= (read (string true)) true) "read(string(true)) roundtrip")
