(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# Helper for asserting errors
(defn assert-err [thunk msg]
  "Assert that (thunk) signals an error"
  (let ([result (try (begin (thunk) :no-error)
                  (catch (e) :got-error))])
    (assert-eq result :got-error msg)))

# ============================================================================
# Conversion primitive tests
# Migrated from tests/property/convert.rs
# ============================================================================

# integer_from_int_is_identity
# (integer n) should return n unchanged
(assert-eq (integer 42) 42 "integer(42) == 42")
(assert-eq (integer 0) 0 "integer(0) == 0")
(assert-eq (integer -99) -99 "integer(-99) == -99")

# integer_from_string_roundtrip
# (integer "n") should parse the string to an integer
(assert-eq (integer "42") 42 "integer(\"42\") == 42")
(assert-eq (integer "0") 0 "integer(\"0\") == 0")
(assert-eq (integer "-99") -99 "integer(\"-99\") == -99")

# float_from_int_preserves_value
# (float n) should convert integer to float
(assert-true (float? (float 42)) "float(42) is a float")
(assert-true (float? (float 0)) "float(0) is a float")
(assert-true (float? (float -99)) "float(-99) is a float")

# number_to_string_int_roundtrip
# (string->integer (number->string n)) should equal n
(assert-eq (string->integer (number->string 42)) 42
           "string->integer(number->string(42)) == 42")
(assert-eq (string->integer (number->string 0)) 0
           "string->integer(number->string(0)) == 0")
(assert-eq (string->integer (number->string -99)) -99
           "string->integer(number->string(-99)) == -99")

# string_from_int_matches_format
# (string n) should format integer as string
(assert-string-eq (string 42) "42" "string(42) == \"42\"")
(assert-string-eq (string 0) "0" "string(0) == \"0\"")
(assert-string-eq (string -99) "-99" "string(-99) == \"-99\"")

# string_from_bool
# (string bool) should format boolean as string
(assert-string-eq (string true) "true" "string(true) == \"true\"")
(assert-string-eq (string false) "false" "string(false) == \"false\"")

# integer_from_float_truncates
# (integer (float n)) should truncate float to integer
(assert-eq (integer (float 42)) 42 "integer(float(42)) == 42")
(assert-eq (integer (float -7)) -7 "integer(float(-7)) == -7")
(assert-eq (integer (float 0)) 0 "integer(float(0)) == 0")

# keyword_to_string
# (string :kw) should convert keyword to string
(assert-string-eq (string :hello) "hello"
                  "string(:hello) == \"hello\"")
(assert-string-eq (string :x) "x"
                  "string(:x) == \"x\"")

# any_to_string
# (any->string val) should convert any value to string representation
(assert-string-eq (any->string nil) "nil" "any->string(nil) == \"nil\"")

# ============================================================================
# string variadic (Issue #495)
# ============================================================================

# Zero arguments returns empty string
(assert-string-eq (string) "" "string() == \"\"")

# Single argument backward compatibility
(assert-string-eq (string 42) "42" "string(42) == \"42\" (backward compat)")
(assert-string-eq (string "hello") "hello" "string(\"hello\") == \"hello\" (backward compat)")
(assert-string-eq (string true) "true" "string(true) (backward compat)")

# Multiple arguments concatenate
(assert-string-eq (string "count: " 42) "count: 42"
  "string multi: string + int")
(assert-string-eq (string "hello" " " "world") "hello world"
  "string multi: three strings")
(assert-string-eq (string 1 " + " 2 " = " 3) "1 + 2 = 3"
  "string multi: mixed types")
(assert-string-eq (string "bool: " true ", nil: " nil) "bool: true, nil: nil"
  "string multi: bool and nil")
(assert-string-eq (string "kw: " :hello) "kw: hello"
  "string multi: keyword")
