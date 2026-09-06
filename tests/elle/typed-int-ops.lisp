(elle/epoch 12)
# audited: 2026-09-06
# The operand proof, end to end.
#
# A %-intrinsic whose operands the front end proved are integers emits the
# integer-only bytecode; one it could not prove emits the polymorphic bytecode.
# Both compute the same answer, on every tier this file runs on.
# docs/impl/lir.md

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

# The instruction set has no RemInt, and the bitwise opcodes already read their
# operands as integers, so a proof buys nothing and changes nothing.
(assert (= (rem-ints) 1) "%rem over proven ints is unchanged")
(assert (= (and-ints) 8) "%bit-and over proven ints is unchanged")
