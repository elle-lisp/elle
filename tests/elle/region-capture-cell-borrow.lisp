(elle/epoch 12)
# Reading a captured local after the closure that captures it
# (docs/impl/region/bindings.md; docs/impl/region/model.md § "Page recycling").
#
# A `def` inside a function body that a nested lambda captures is materialized
# as a per-value env cell, and the enclosing function then reads the value back
# out of that cell rather than out of a local slot. Every read below is such a
# cell read, and each one's result is consumed by a further call — an uncounted
# borrow whose reader runs after the read. The value must still be live when
# that reader runs, whatever the closure does or does not do with its capture.
#
# The shapes differ in what stands between the capture and the read:
#
#   - the closure is never called, so nothing but its construction touches the
#     captured binding;
#   - the closure is called first, so the cell has been read through the
#     closure as well;
#   - the read's result feeds an outer call, a `let` binding, and a discarded
#     statement position, which are three different consumers.
#
# ── Running this under the scrub amplifier ────────────────────────────────
# A read through a pointer that outlived its region normally returns the dead
# region's bytes, which for a short-lived program are still intact and still
# well-typed — so a released-too-early value reads correctly and this file
# passes without proving the release was late enough. `--trace=scrub` removes
# that cover: it blanks a released page's body, so the same read lands on an
# all-zero slot and `arena::deref` panics at the deref site
# (docs/impl/region/diagnostics.md).
#
#     elle --trace=scrub tests/elle/region-capture-cell-borrow.lisp
#
# Run it that way when changing capture-cell release placement. In the plain
# corpus this file pins the results only; `tests/region_cell_borrow.rs` runs the
# same shapes with scrub armed, so the gate carries the amplified check.

(defn uncalled-closure []
  "The captured binding is read after a closure captures it, never calling it."
  (def vals [10 20])
  (defn peek []
    (get vals 0))
  (type (get vals 0)))

(assert (= (uncalled-closure) :integer)
        "read after an uncalled capturing closure")

(defn called-closure []
  "Same, with the closure called before the read."
  (def vals [10 20])
  (defn peek []
    (get vals 0))
  (peek)
  (peek)
  (type (get vals 0)))

(assert (= (called-closure) :integer) "read after a called capturing closure")

(defn read-into-let []
  "The read's consumer is a `let` binding rather than an enclosing call."
  (def vals [10 20])
  (defn peek []
    vals)
  (let [x (get vals 1)]
    (+ x 1)))

(assert (= (read-into-let) 21) "read bound by a let after a capture")

(defn read-in-statement []
  "The read's value is discarded, so the only consumer is the read itself."
  (def vals [10 20])
  (defn peek []
    vals)
  (begin
    (get vals 0)
    99))

(assert (= (read-in-statement) 99) "discarded read after a capture")

(defn read-through-length []
  "A different reader native, so the shape is not `get`-specific."
  (def vals [10 20])
  (defn peek []
    vals)
  (type (length vals)))

(assert (= (read-through-length) :integer) "length read after a capture")

(defn nested-letrec []
  "The capturing closure holds a letrec, the shape contracts.lisp uses."
  (def vals (map (fn [p] {:check p}) [integer?]))
  (defn checker []
    (letrec [check (fn [i]
                     (when (< i 1)
                       (let [v (get vals i)]
                         (type v))
                       (check (+ i 1))))]
      (check 0)))
  (checker)
  (checker)
  (type (get vals 0)))

(assert (= (nested-letrec) :struct) "letrec capture, then read")

(println "region-capture-cell-borrow: ok")
