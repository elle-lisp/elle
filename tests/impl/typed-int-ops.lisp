(elle/epoch 12)
# audited: 2026-09-09
# The operand proof, end to end.
#
# A %-intrinsic whose operands the front end proved are integers emits the
# integer-only bytecode; one it could not prove emits the polymorphic bytecode.
# Both compute the same answer, on every tier this file runs on.
#
# A proven op's result carries a type of its own, so the proof reaches the next
# op in the chain — and a float operand denies it there. A call's result carries
# one too, a recursive call included.
# docs/impl/lir.md
# docs/intrinsics.md

(defn disasm-text [f]
  "The disassembly of f's bytecode, as one string."
  (string/join (fn/disasm f) " "))

# ── Two int literals prove the operation ─────────────────────────

(defn add-ints []
  (%add 6 7))
(defn sub-ints []
  (%sub 6 7))
(defn mul-ints []
  (%mul 6 7))
(defn div-ints []
  (%div 6 7))

(assert (string/contains? (disasm-text add-ints) "AddInt")
        "%add over ints emits AddInt")
(assert (string/contains? (disasm-text sub-ints) "SubInt")
        "%sub over ints emits SubInt")
(assert (string/contains? (disasm-text mul-ints) "MulInt")
        "%mul over ints emits MulInt")
(assert (string/contains? (disasm-text div-ints) "DivInt")
        "%div over ints emits DivInt")

(assert (= (add-ints) 13) "AddInt computes the sum")
(assert (= (sub-ints) -1) "SubInt computes the difference")
(assert (= (mul-ints) 42) "MulInt computes the product")
(assert (= (div-ints) 0) "DivInt truncates toward zero")

# ── Floats are not integers ──────────────────────────────────────

(defn add-floats []
  (%add 6.5 7.5))

# The counter-factual. Both operands are proven Numbers, so the site compiles;
# an emitter that read "proven" as "proven int" would give a float pair to
# integer wrapping arithmetic and return garbage instead of 14.0.
(assert (not (string/contains? (disasm-text add-floats) "AddInt"))
        "%add over floats keeps the polymorphic Add")
(assert (= (add-floats) 14.0) "the polymorphic Add adds floats")

# ── A Number proof is not an Int proof ───────────────────────────

(defn square [x]
  "(numeric!) floors the parameter at Number, which admits either width."
  (numeric!)
  (%mul x x))

(assert (not (string/contains? (disasm-text square) "MulInt"))
        "a Number-proven operand does not select the integer opcode")
(assert (= (square 7) 49) "the polymorphic Mul squares an int")
(assert (= (square 1.5) 2.25) "and the same code squares a float")

# ── A guard narrows a parameter to Int ───────────────────────────

(defn bump [x]
  "A diverging guard proves x is an int in everything below it."
  (when (%not (int? x))
    (error {:error :type-error :message "bump: int required"}))
  (%add x 1))

(assert (string/contains? (disasm-text bump) "AddInt")
        "a guard-narrowed int parameter proves the operation")
(assert (= (bump 41) 42) "the guarded fast path computes the sum")
(assert (= (bump -1) 0) "and holds across zero")

# ── The unspecialized operations are unchanged ───────────────────

(defn rem-ints []
  (%rem 7 3))
(defn and-ints []
  (%bit-and 12 10))

# The bytecode set has no RemInt, and the bitwise opcodes already read their
# operands as integers, so the proof selects no different opcode here. It is
# not idle: the JIT spends it on Rem (dropping the tag check, keeping the zero
# test), and the next section spends it downstream on every tier.
(assert (= (rem-ints) 1) "%rem over proven ints is unchanged")
(assert (= (and-ints) 8) "%bit-and over proven ints is unchanged")

# ── The result of a proven operation proves the next one ─────────

(defn rem-then-add [x]
  "The %rem result is the %add's proof: one guard carries the whole chain."
  (when (%not (int? x))
    (error {:error :type-error :message "rem-then-add: int required"}))
  (%add (%rem x 16) 1))

# The counter-factual, and the reason this section exists: while a remainder
# typed as Number, the %add read an unproven operand and kept the polymorphic
# Add — the int chain died at the remainder even though both its operands were
# proven ints.
(assert (string/contains? (disasm-text rem-then-add) "AddInt")
        "the remainder of two proven ints proves the %add")
(assert (= (rem-then-add 33) 2) "AddInt computes over the remainder")

(defn mask [x]
  "The masking kernel the bitwise contract used to refuse."
  (when (%not (int? x))
    (error {:error :type-error :message "mask: int required"}))
  (%bit-and (%rem x 16) 15))

(assert (= (mask 33) 1) "a guard-proven remainder discharges %bit-and")
(assert (= (mask 255) 15) "and holds at the mask's top value")

# ── A recursive call's result proves the next operation ──────────

(defn fib-int [n]
  "The recursion's own result is the addition's proof: a self-call reads the
   body type the previous pass of the ascent computed."
  (when (%not (int? n))
    (error {:error :type-error :message "fib-int: int required"}))
  (if (%lt n 2)
    n
    (%add (fib-int (%sub n 1)) (fib-int (%sub n 2)))))

# The counter-factual: while a self-recursive call returned Bottom, this %add
# read two unproven operands and kept the polymorphic Add. No spelling of an
# integer recursion emitted the integer opcode, and the JIT kept a tag-check
# diamond in the hot path of a body that is two subtractions and a compare.
(assert (string/contains? (disasm-text fib-int) "AddInt")
        "%add over two self-recursive int results emits AddInt")
(assert (= (fib-int 10) 55) "AddInt computes the recursion")
(assert (= (fib-int 0) 0) "and the base case returns its argument")

# The proof follows the body, not the shape: a base case that returns a float
# makes the recursive result a Number, which the bitwise contract refuses.
(let [r (protect (compile/barrier-module (string "(defn f [n] "
                 "  (if (%lt n 2) 1.5 (%bit-and (%add (f (%sub n 1)) 1) 3))) "
                 "(f 4)") "<typed-int-ops>"))]
  (assert (not (get r 0)) "a float base case must not prove an int result")
  (assert (string/contains? (get (get r 1) :message) "%bit-and")
          (string "the diagnostic must name the refusing op, got "
                  (get (get r 1) :message))))

# ── A reassigned lambda binding proves nothing ───────────────────

(defn call-local-lambda []
  "One initializer and no write, so the call reads the lambda's body type."
  (var g (fn [x] (%add x 1)))
  (%add (g 1) 1))

(assert (string/contains? (disasm-text call-local-lambda) "AddInt")
        "a call to an unwritten local lambda binding proves the %add")
(assert (= (call-local-lambda) 3) "AddInt computes over the call's result")

# The counter-factual: the binder kept the first initializer's body type
# through every later write, so this compiled and `%bit-and` ran the integer
# opcode over the string "s", printing 0.
(let [r (protect (compile/barrier-module (string "(var f (fn [x] 1)) "
                 "(assign f (fn [x] \"s\")) " "(%bit-and (f 0) 1)")
                 "<typed-int-ops>"))]
  (assert (not (get r 0))
          "a reassigned lambda binding must not prove its result")
  (assert (string/contains? (get (get r 1) :message) "%bit-and")
          (string "the diagnostic must name the refusing op, got "
                  (get (get r 1) :message))))

# ── A float operand proves nothing to a bitwise op ───────────────

# The trap: a bitwise opcode reads a float's payload as an integer, so a wrong
# result type here is silent garbage rather than a fault.
#
# The counter-factual: while %mod was declared the constant Int this compiled,
# and `(%bit-and (%mod 5.5 2.0) 1)` returned 0 — where the `mod` wrapper's own
# integer? guards raise :type-error on the same operands.
(let [r (protect (compile/barrier-module "(%bit-and (%mod 5.5 2.0) 1)"
                 "<typed-int-ops>"))]
  (assert (not (get r 0)) "a float %mod under a bitwise op must not compile")
  (assert (= (get (get r 1) :error) :compile-error)
          (string "expected :compile-error, got " (get r 1)))
  (assert (string/contains? (get (get r 1) :message) "%bit-and")
          (string "the diagnostic must name the refusing op, got "
                  (get (get r 1) :message)))
  (assert (string/contains? (get (get r 1) :message) "not a proven int")
          (string "the diagnostic must say what was unproven, got "
                  (get (get r 1) :message))))

# The refusal is the bitwise op's, not %mod's: the float domain still computes.
(assert (= (%mod 5.5 2.0) 1.5) "%mod over floats is a float")
(assert (= (%mod -5 3) 1) "%mod over ints floors toward the divisor's sign")
