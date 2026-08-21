(elle/epoch 12)
# A reassigned mutable binding fed by a CALL RESULT holds a counted reference
# (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
# containers"; docs/impl/region/mechanism.md § "The return mint is emitted
# exactly once").
#
# The 1-slot-container model gives the cell ONE reference to its current content
# and drops it at the next overwrite. Where that content is a fresh local
# allocation the cell may simply take over the producer's birth reference (the
# DONATION), but a CALL RESULT already carries its own release — the opaque
# placeholder region the lowerer frees by value through the ANF temp's slot — so
# the cell must take a COUNTED reference instead: donating on top of that release
# would leave the cell pointing at a freed value.
#
# Two properties, both checked here under `--trace=guardfree` (the only
# trustworthy UAF oracle):
#
#   * the counted store happens — the callee's one returned reference plus the
#     cell's incref survive the ANF temp's release, so the slot's value is live
#     for every later read;
#   * the retain lands on the STORED value — `StoreLocal` consumes the value
#     register, so the retain must be emitted BEFORE it. Emitted after, it pins
#     whatever sits on the operand stack instead (the displaced prior, `nil` on
#     the first overwrite), and the stored value dies at the ANF temp's release.
#     The first iteration's overwrite is exactly that `nil` case, which is why
#     the loops below start from a `nil`-initialised cell.
#
# The subjects vary who produces the value and how deep the return travels: a
# direct callee, one that forwards through a bare tail call (the callee's frame
# is replaced, so the inner function's return convention is what reaches the
# cell), and the stdlib HOF pipeline. Heap ELEMENTS (fresh strings, not
# immediates) so a premature free is a real dangling read, and enough iterations
# that a recycled page would surface as wrong data even without guardfree.

(defn build [n]
  (let [acc @[]]
    (push acc (concat "v" (number->string n)))
    (freeze acc)))

(defn forward [n]
  (build n))

# ── subject 1: module-scope cell fed by a direct call result ───────────────────
(var direct nil)
(var i 0)
(while (%lt i 200)
  (assign direct (build i))
  (assign i (%add i 1)))
(assert (= (length direct) 1)
        "module-scope cell fed by a call result: content freed under the cell")
(assert (= (get direct 0) "v199")
        "module-scope cell fed by a call result: content is not the last stored")

# ── subject 2: the value returns through a bare tail call ─────────────────────
(var fwd nil)
(var j 0)
(while (%lt j 200)
  (assign fwd (forward j))
  (assign j (%add j 1)))
(assert (= (get fwd 0) "v199")
        "cell fed through a tail-forwarding callee: content freed or stale")

# ── subject 3: the value is a stdlib HOF result ───────────────────────────────
# `xs` is a parameter, so the collection is not a statically-proven array and the
# HOF stays a real call (loop fusion would otherwise dissolve it into an inlined
# walk, leaving no call result for the cell to hold).
(defn mapper [xs]
  (map (fn [x] (concat "m" x)) xs))
(var mapped nil)
(var k 0)
(while (%lt k 200)
  (assign mapped (mapper ["a" "b"]))
  (assign k (%add k 1)))
(assert (= (length mapped) 2) "cell fed by a HOF result: content freed")
(assert (= (get mapped 1) "mb") "cell fed by a HOF result: content is stale")

# ── control: the same cell fed by a FRESH LOCAL allocation (the donation path,
# which must keep working — the cell takes over the producer's birth reference
# and no incref-on-store is added). ────────────────────────────────────────────
(var fresh nil)
(var n 0)
(while (%lt n 200)
  (assign fresh (pair n (concat "f" (number->string n))))
  (assign n (%add n 1)))
(assert (= (first fresh) 199) "donation control: fresh-allocation cell is stale")

(println "region-reassign-callresult-store: ok")
