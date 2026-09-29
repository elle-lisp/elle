(elle/epoch 12)
# Test: one dead operand, popped once per path — never twice
#
# The bytecode emitter simulates the operand stack per block, and a block
# inherits its starting simulation from the first predecessor that reaches it.
# That first predecessor fixes the merge block's operand depth; every later
# edge must arrive at the same depth (src/lir/AGENTS.md "Merge operand depth").
#
# A reassignment of a loop-carried mutable leaves one dead operand behind: the
# store copies the new value to the top of the stack with DupN, past the old
# value it is about to release, and the original cell is orphaned once the copy
# is consumed. That orphan then sits under everything the rest of the loop body
# pushes.
#
# The emitter drops such orphans on a jump edge. Dropping one past the depth a
# sibling branch edge left behind is what this file pins: the branch edge into
# the merge keeps the orphan, the jump edge removes it, and the merge's own arms
# — which inherited the branch's simulation — remove it a second time on the
# path that already did. The operand stack then falls one slot into the region
# the frame reserved for its locals, and the topmost local stops existing:
# "VM bug: Local variable index out of bounds".

# ── minimal reproduction ──────────────────────────────────
#
# `(and ...)` short-circuits on its second operand, so the `when` takes its
# empty else arm — two jump edges on one path, one orphan between them.

(defn count-to [limit n]
  (let [@turns 0
        @flagged 0]
    (while (< turns limit)
      (assign turns (+ turns 1))
      (when (and (> n 0) (> n 100)) (assign flagged 1)))
    [turns flagged]))

(assert (= (count-to 3 1) [3 0]) "loop with a short-circuiting when survives")
(assert (= (count-to 1 1) [1 0]) "one iteration is enough to hit it")
(assert (= (count-to 3 200) [3 1]) "the taken arm still assigns")
(assert (= (count-to 0 1) [0 0]) "a loop that never runs is unaffected")

# ── variations ────────────────────────────────────────────

# Two conditionals in the body: each arm is another edge into another merge.
(defn twice [n]
  (let [@turns 0
        @a 0
        @b 0]
    (while (< turns 3)
      (assign turns (+ turns 1))
      (when (and (> n 0) (> n 100)) (assign a 1))
      (when (and (> n 0) (> n 200)) (assign b 1)))
    [turns a b]))

(assert (= (twice 1) [3 0 0]) "two short-circuiting whens in one body")
(assert (= (twice 150) [3 1 0]) "first arm taken, second not")
(assert (= (twice 250) [3 1 1]) "both arms taken")

# `or` merges the same way, with the arms inverted.
(defn with-or [n]
  (let [@turns 0
        @flagged 0]
    (while (< turns 3)
      (assign turns (+ turns 1))
      (when (or (> n 100) (> n 200)) (assign flagged 1)))
    [turns flagged]))

(assert (= (with-or 1) [3 0]) "or short-circuits without losing a local")
(assert (= (with-or 150) [3 1]) "or's first operand still selects the arm")

# A heap-valued mutable takes the same reassignment path.
(defn accumulate [n]
  (let [@acc @[]
        @turns 0
        @flagged 0]
    (while (< turns 3)
      (assign turns (+ turns 1))
      (assign acc (thaw (push (freeze acc) turns)))
      (when (and (> n 0) (> n 100)) (assign flagged 1)))
    [(length acc) flagged]))

(assert (= (accumulate 1) [3 0]) "heap-valued reassignment keeps its locals")

# Nested: the inner loop's merges sit inside the outer body's.
(defn nested [n]
  (let [@outer 0
        @inner 0
        @flagged 0]
    (while (< outer 2)
      (assign outer (+ outer 1))
      (while (< inner 3)
        (assign inner (+ inner 1))
        (when (and (> n 0) (> n 100)) (assign flagged 1))))
    [outer inner flagged]))

(assert (= (nested 1) [2 3 0]) "nested loops keep their locals")

(println "orphan-double-pop: all tests passed")
