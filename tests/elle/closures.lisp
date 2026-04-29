(elle/epoch 9)
# Closure cell_locals_mask optimization
#
# The VM avoids wrapping locally-defined variables in LocalCell when
# they are not captured by nested closures.  Non-cell locals use stack
# slots (StoreLocal/LoadLocal) instead of env slots (StoreUpvalue/
# LoadUpvalue).  These tests verify the optimization preserves correct
# behavior at the boundary between cell-wrapped and non-cell locals.


# ============================================================================
# Non-captured let bindings — use stack, no heap allocation
# ============================================================================

# Non-captured, non-mutated let binding
(assert (= (let [x 42]
             x) 42) "non-captured let binding")

# Non-captured let inside a lambda (the optimization target)
(assert (= ((fn ()
              (let [x 10]
                (+ x 5)))) 15) "non-captured let in lambda")

# ============================================================================
# Captured + mutated let — must still use LocalCell
# ============================================================================

# Counter closure that increments a captured mutable binding
(let [counter ((fn ()
                 (let [@n 0]
                   (fn ()
                     (assign n (+ n 1))
                     n))))]
  (counter)
  (counter)
  (assert (= (counter) 3) "captured mutated let uses LocalCell"))

# ============================================================================
# Mutated but not captured — uses StoreLocal for set
# ============================================================================

(assert (= ((fn ()
              (let [@y 0]
                (assign y 10)
                y))) 10) "mutated non-captured let in lambda")

# ============================================================================
# Mixed cell needs — both paths in the same lambda
# ============================================================================

# a and c are not captured; b is captured by get-b
(assert (= ((fn ()
              (let [a 1]
                (let [b 2]
                  (let [c 3]
                    (let [get-b (fn () b)]
                      (+ a (get-b) c))))))) 6)
        "mixed captured and non-captured lets")

# ============================================================================
# letrec with self-recursive binding
# ============================================================================

# Factorial via letrec — the binding is captured by its own body
(assert (= ((fn ()
              (letrec [f (fn (n)
                           (if (= n 0)
                             1
                             (* n (f (- n 1)))))]
                (f 5)))) 120) "letrec factorial")
