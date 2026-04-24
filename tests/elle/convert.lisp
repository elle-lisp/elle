(elle/epoch 9)

# Helper for asserting errors
(defn assert-err [thunk msg]
  "Assert that (thunk) signals an error"
  (let [[ok? _] (protect (thunk))]
    (assert (not ok?) msg)))

# ============================================================================
# Conversion primitive tests
# Migrated from tests/property/convert.rs
# ============================================================================

# integer_from_int_is_identity
# (integer n) should return n unchanged
(assert (= (integer 42) 42) "integer(42) == 42")
(assert (= (integer 0) 0) "integer(0) == 0")
(assert (= (integer -99) -99) "integer(-99) == -99")

# integer_from_string_roundtrip
# (parse-int "n") should parse the string to an integer
(assert (= (parse-int "42") 42) "integer(\"42\") == 42")
(assert (= (parse-int "0") 0) "integer(\"0\") == 0")
(assert (= (parse-int "-99") -99) "integer(\"-99\") == -99")

# float_from_int_preserves_value
# (float n) should convert integer to float
(assert (float? (float 42)) "float(42) is a float")
(assert (float? (float 0)) "float(0) is a float")
(assert (float? (float -99)) "float(-99) is a float")

# number_to_string_int_roundtrip
# (parse-int (number->string n)) should equal n
(assert (= (parse-int (number->string 42)) 42) "integer(number->string(42)) == 42")
(assert (= (parse-int (number->string 0)) 0) "integer(number->string(0)) == 0")
(assert (= (parse-int (number->string -99)) -99) "integer(number->string(-99)) == -99")

# string_from_int_matches_format
# (string n) should format integer as string
(assert (= (string 42) "42") "string(42) == \"42\"")
(assert (= (string 0) "0") "string(0) == \"0\"")
(assert (= (string -99) "-99") "string(-99) == \"-99\"")

# string_from_bool
# (string bool) should format boolean as string
(assert (= (string true) "true") "string(true) == \"true\"")
(assert (= (string false) "false") "string(false) == \"false\"")

# integer_from_float_truncates
# (integer (float n)) should truncate float to integer
(assert (= (integer (float 42)) 42) "integer(float(42)) == 42")
(assert (= (integer (float -7)) -7) "integer(float(-7)) == -7")
(assert (= (integer (float 0)) 0) "integer(float(0)) == 0")

# keyword_to_string
# (string :kw) should convert keyword to string
(assert (= (string :hello) "hello") "string(:hello) == \"hello\"")
(assert (= (string :x) "x") "string(:x) == \"x\"")

# any_to_string
# (any->string val) should convert any value to string representation
(assert (= (any->string nil) "nil") "any->string(nil) == \"nil\"")

# ============================================================================
# number->string with radix
# ============================================================================

# Basic radix conversions
(assert (= (number->string 255 16) "ff") "hex 255")
(assert (= (number->string 255 2) "11111111") "binary 255")
(assert (= (number->string 255 8) "377") "octal 255")
(assert (= (number->string 10 10) "10") "decimal explicit")
(assert (= (number->string 35 36) "z") "base 36")
(assert (= (number->string -255 16) "-ff") "negative hex")
(assert (= (number->string 0 16) "0") "zero hex")

# Backward compatibility: 1-arg still works
(assert (= (number->string 42) "42") "no radix still works")

# Error cases
(let [[ok? _] (protect ((fn () (number->string 3.14 16))))] (assert (not ok?) "float with radix errors"))
(let [[ok? _] (protect ((fn () (number->string 42 1))))] (assert (not ok?) "radix 1 errors"))
(let [[ok? _] (protect ((fn () (number->string 42 37))))] (assert (not ok?) "radix 37 errors"))
(let [[ok? _] (protect ((fn () (number->string "hello"))))] (assert (not ok?) "non-number errors"))

# ============================================================================
# string variadic (Issue #495)
# ============================================================================

# Zero arguments returns empty string
(assert (= (string) "") "string() == \"\"")

# Single argument backward compatibility
(assert (= (string 42) "42") "string(42) == \"42\" (backward compat)")
(assert (= (string "hello") "hello") "string(\"hello\") == \"hello\" (backward compat)")
(assert (= (string true) "true") "string(true) (backward compat)")

# Multiple arguments concatenate
(assert (= (string "count: " 42) "count: 42") "string multi: string + int")
(assert (= (string "hello" " " "world") "hello world") "string multi: three strings")
(assert (= (string 1 " + " 2 " = " 3) "1 + 2 = 3") "string multi: mixed types")
(assert (= (string "bool: " true ", nil: " nil) "bool: true, nil: nil") "string multi: bool and nil")
(assert (= (string "kw: " :hello) "kw: hello") "string multi: keyword")
