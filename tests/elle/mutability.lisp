(elle/epoch 10)
# Mutability regression tests
#
# Guards against assign-in-dead-branch executing unconditionally.

# ── Assign in dead if-then branch (with begin) ───────────────

# test_assign_dead_branch_begin
# (when cond body) expands to (if cond (begin body) nil).
# Assign inside the begin must NOT execute when condition is false.
(def @x 1)
(if (> 0 1)
  (begin
    (assign x 99))
  nil)
(assert (= x 1) "assign in dead if-then+begin should not execute")

# test_assign_dead_when
(def @y 1)
(when (> 0 1) (assign y 99))
(assert (= y 1) "assign in dead when should not execute")

# test_assign_live_branch_begin
(def @z 1)
(if (> 1 0)
  (begin
    (assign z 99))
  nil)
(assert (= z 99) "assign in live if-then+begin should execute")

# test_assign_live_when
(def @w 1)
(when (> 1 0) (assign w 99))
(assert (= w 99) "assign in live when should execute")

# ── Assign without begin (always worked) ─────────────────────

# test_assign_dead_branch_no_begin
(def @a 1)
(if (> 0 1) (assign a 99) nil)
(assert (= a 1) "assign in dead if-then (no begin) stays 1")

# ── Let-scoped mutable (always worked) ───────────────────────

# test_let_scoped_assign_dead_branch
(let [@b 1]
  (if (> 0 1)
    (begin
      (assign b 99))
    nil)
  (assert (= b 1) "let-scoped assign in dead branch stays 1"))

# ── Multiple assigns in branches ──────────────────────────────

# test_multiple_assigns_correct_branch
(def @p 0)
(def @q 0)
(if (> 1 0)
  (begin
    (assign p 10)
    (assign q 20))
  (begin
    (assign p 30)
    (assign q 40)))
(assert (= p 10) "then-branch assign p")
(assert (= q 20) "then-branch assign q")

# test_multiple_assigns_else_branch
(def @r 0)
(def @s 0)
(if (> 0 1)
  (begin
    (assign r 10)
    (assign s 20))
  (begin
    (assign r 30)
    (assign s 40)))
(assert (= r 30) "else-branch assign r")
(assert (= s 40) "else-branch assign s")

# ── Nested if with assigns ────────────────────────────────────

# test_nested_if_assigns
(def @n 0)
(if true (if false (assign n 1) (assign n 2)) (assign n 3))
(assert (= n 2) "nested if assign correct branch")
