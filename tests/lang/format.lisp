(elle/epoch 18)
# audited: 2026-11-20
# string/format: positional and named substitution, format specifications, how a value renders, and the errors.
# src/primitives/format/AGENTS.md

# ============================================================================
# string/format — positional interpolation
# ============================================================================

(assert (= (string/format "{} + {} = {}" 1 2 3) "1 + 2 = 3")
        "format positional basic")

(assert (= (string/format "Hello, {}!" "Alice") "Hello, Alice!")
        "format positional with string")

(assert (= (string/format "{}" 42) "42") "format positional single")

(assert (= (string/format "no placeholders") "no placeholders")
        "format no placeholders")

(assert (= (string/format "literal {{braces}}") "literal {braces}")
        "format escaped braces")

# ============================================================================
# string/format — named interpolation
# ============================================================================

(assert (= (string/format "{name} is {age}" :name "Alice" :age 30) "Alice is 30")
        "format named basic")

(assert (= (string/format "{greeting}, {name}!" :greeting "Hello" :name "Bob")
           "Hello, Bob!") "format named multiple")

# ============================================================================
# string/format — format specs
# ============================================================================

(assert (= (string/format "{:.2f}" 3.14159) "3.14") "format float precision")

(assert (= (string/format "{:>10}" "hello") "     hello") "format right align")

(assert (= (string/format "{:<10}" "hello") "hello     ") "format left align")

(assert (= (string/format "{:^10}" "hello") "  hello   ") "format center align")

(assert (= (string/format "{:05d}" 42) "00042") "format zero padding")

(assert (= (string/format "{:x}" 255) "ff") "format hex lowercase")

(assert (= (string/format "{:X}" 255) "FF") "format hex uppercase")

(assert (= (string/format "{:o}" 8) "10") "format octal")

(assert (= (string/format "{:b}" 10) "1010") "format binary")

(assert (= (string/format "{:e}" 1000.0) "1e3") "format scientific")

(assert (= (string/format "{:*^10}" "hi") "****hi****")
        "format custom fill center")

(assert (= (string/format "{:>10d}" 42) "        42")
        "format right align integer")

# Default alignment: numbers right-align, strings left-align
(assert (= (string/format "{:10}" 42) "        42")
        "format default align integer (right)")
(assert (= (string/format "{:10}" "hi") "hi        ")
        "format default align string (left)")

# Named with format specs
(assert (= (string/format "{val:.2f}" :val 3.14159) "3.14")
        "format named with spec")

# ============================================================================
# string/format — error cases
# ============================================================================

# Positional arg count mismatch
(let [[ok? _] (protect ((fn [] (string/format "{} {}" 1))))]
  (assert (not ok?) "format positional too few args"))

(let [[ok? _] (protect ((fn [] (string/format "{}" 1 2))))]
  (assert (not ok?) "format positional too many args"))

# Named arg missing
(let [[ok? _] (protect ((fn [] (string/format "{name}" :other "value"))))]
  (assert (not ok?) "format named missing key"))

# Named arg extra
(let [[ok? _] (protect ((fn []
                          (string/format "{name}" :name "Alice" :extra "Bob"))))]
  (assert (not ok?) "format named extra key"))

# Mixed positional and named
(let [[ok? _] (protect ((fn [] (string/format "{} {name}" 1 :name "Alice"))))]
  (assert (not ok?) "format mixed positional and named"))

# Odd keyword args
(let [[ok? _] (protect ((fn [] (string/format "{name}" :name))))]
  (assert (not ok?) "format odd keyword args"))

# Non-keyword in named position
(let [[ok? _] (protect ((fn [] (string/format "{name}" 42 "Alice"))))]
  (assert (not ok?) "format non-keyword in named position"))

# Template not a string
(let [[ok? _] (protect ((fn [] (string/format 42))))]
  (assert (not ok?) "format template not string"))

# Invalid format spec
(let [[ok? _] (protect ((fn [] (string/format "{:z}" 42))))]
  (assert (not ok?) "format invalid spec"))

# Type mismatch: string with integer spec
(let [[ok? _] (protect ((fn [] (string/format "{:d}" "hello"))))]
  (assert (not ok?) "format type mismatch string as d"))

# ============================================================================
# string/format — how a value renders
# ============================================================================

# A placeholder with no type renders a value as `string` does. The
# counter-factual wrote the value with no symbol table, so a keyword showed
# its hash, `#<keyword:0x…>`, where `string` and `println` show its name.
(assert (= (string/format "{}" :zork-format) "zork-format")
        "format a keyword by its name")
(assert (= (string/format "{}" [:zork-format 1]) (string [:zork-format 1]))
        "format a collection of keywords as string does")
(assert (= (string/format "{kind}" :kind :zork-format) "zork-format")
        "format a named keyword by its name")
