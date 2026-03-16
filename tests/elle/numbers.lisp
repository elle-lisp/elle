## Numeric Literal Tests (#540)
##
## Tests for hexadecimal, octal, binary, underscore, and scientific notation
## literals. All forms parse to the same Value::int or Value::float as their
## decimal equivalents.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Hexadecimal integer literals
# ============================================================================

(assert-eq 0xFF 255 "hex literal 0xFF")
(assert-eq 0XFF 255 "hex literal 0XFF uppercase prefix")
(assert-eq 0xff 255 "hex literal 0xff lowercase digits")
(assert-eq 0x0 0 "hex literal zero")
(assert-eq 0x1A2B 6699 "hex literal 0x1A2B")
(assert-eq (+ 0xFF 1) 256 "hex in arithmetic")

# ============================================================================
# Octal integer literals
# ============================================================================

(assert-eq 0o755 493 "octal literal 0o755")
(assert-eq 0O755 493 "octal literal 0O755 uppercase prefix")
(assert-eq 0o0 0 "octal literal zero")
(assert-eq 0o644 420 "octal literal 0o644")

# ============================================================================
# Binary integer literals
# ============================================================================

(assert-eq 0b1010 10 "binary literal 0b1010")
(assert-eq 0B1010 10 "binary literal 0B1010 uppercase prefix")
(assert-eq 0b0 0 "binary literal zero")
(assert-eq 0b11110000 240 "binary literal 0b11110000")
(assert-eq (bit/and 0b1111 0b1010) 0b1010 "binary in bitwise ops")

# ============================================================================
# Underscores in integer literals
# ============================================================================

(assert-eq 1_000_000 1000000 "underscore in decimal integer")
(assert-eq 0xFF_FF 65535 "underscore in hex")
(assert-eq 0b1010_1010 170 "underscore in binary")
(assert-eq 0o7_5_5 493 "underscore in octal")

# ============================================================================
# Scientific notation (bug fix: these previously silently broke into 2 tokens)
# ============================================================================

(assert-true (< (- 1.5e10 15000000000.0) 1.0) "scientific notation 1.5e10")
(assert-true (< (- 1e10 10000000000.0) 1.0) "scientific notation 1e10 no dot")
(assert-true (< (abs (- 2.3e-5 0.000023)) 1e-15) "scientific notation 2.3e-5")
(assert-true (< (abs (- 1.5E10 1.5e10)) 1.0) "scientific notation uppercase E")
(assert-true (< (abs (- 1e+10 1e10)) 1.0) "scientific notation explicit positive exponent")

# ============================================================================
# Underscores in float literals
# ============================================================================

(assert-true (< (abs (- 1_000.5_5 1000.55)) 1e-9) "underscore in float")
(assert-true (< (abs (- 1.5e1_0 1.5e10)) 1.0) "underscore in float exponent")

# ============================================================================
# Negative literals
# ============================================================================

(assert-eq -0xFF -255 "negative hex literal")
(assert-eq -0o10 -8 "negative octal literal")
(assert-eq -0b1 -1 "negative binary literal")
(assert-eq -1_000 -1000 "negative with underscore")

# ============================================================================
# Backward compatibility: existing decimal parsing unchanged
# ============================================================================

(assert-eq 42 42 "plain decimal integer unchanged")
(assert-eq -42 -42 "negative decimal unchanged")
(assert-eq 0 0 "zero unchanged")
(assert-eq 042 42 "leading zero stays decimal (not octal)")
