(elle/epoch 12)
# Counterfactual for the tail-call argument-ownership leak.
#
# Calling convention: a callee OWNS its parameters and releases them at
# their last use; a caller transfers one reference per argument (a move on
# the value's last use, an incref when it keeps the value). Before this
# fix, args were borrows the caller released *after* the call — but a tail
# call replaces the frame, so the caller's post-call `DecrefValueRegion`
# was emitted past the `TailCall` (unreachable) and the arg's region leaked
# once per evaluation (the http2 mutable+immutable byte-append leak, via
# `concat`'s `(@string)`/`(@bytes)` accumulator passed to a tail call).
#
# These exercise the corners that must all stay balanced — no leak AND no
# double-free / UAF:
#   - a heap value passed to a tail call whose callee discards it
#   - a heap value passed to a tail call whose callee RETURNS it (aliasing:
#     the returned region must survive)
#   - a heap value used after a non-tail call (caller keeps it)

(defn sink (x)
  7)
(defn ident (x)
  x)
(defn mk (x)
  (string "fresh"))

# (1) bound heap value passed to a tail call that discards it → must not leak.
(defn pass-to-tail (n)
  (let [s (string "x-" n)]
    (sink s)))

# (2) bound heap value passed to a tail call that RETURNS it → must survive
#     (no UAF) and not leak.
(defn pass-through-tail (n)
  (let [s (string "y-" n)]
    (ident s)))

# (3) used after a non-tail call, then a tail call → caller keeps s, no
#     double-free.
(defn keep-then-tail (n)
  (let [s (string "z-" n)]
    (let [_ (sink s)]
      (sink s))))

# (4) a value the tail call's arguments only READ OUT OF. The callee takes
#     over each destructured leaf, never `t` itself, so `t`'s own reference is
#     still the caller's to drop and must be dropped ahead of the frame
#     replacement. Every h2 frame builder is this shape —
#     `(let [[ft fl si pl] (make-…)] (send-frame s ft fl si pl))`.
(defn mk4 (n)
  [(string "a" n) 2 3 4])
(defn sink4 (a b c d)
  7)
(defn leaves-to-tail (n)
  (let [[a b c d] (mk4 n)]
    (sink4 a b c d)))

# (5) the same with ONE leaf reaching the callee: the source has no later use
#     to ride, and the leaf that does reach it is the heap element, so this is
#     the corner where a release hoisted ahead of the call must not free what
#     the callee is about to read.
(defn one-leaf-to-tail (n)
  (let [[a b c d] (mk4 n)]
    (sink a)))

# (6) an ALIAS of the moved value, not a part of it: `w` is a second name for
#     the array the inner `let` built and returned, so the call moves that very
#     reference and the caller must NOT drop it (the stdlib `zip` shape — a
#     double-free if a leaf's reading is applied to a whole-value alias).
(defn build ()
  (let [a @[]]
    (push a "m")
    a))
(defn alias-to-tail (n)
  (let [w (build)]
    (sink w)))

(defn region-delta (f iters)
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# Correctness first: the aliasing case must return the right value.
(assert (= (pass-through-tail 1) "y-1")
        "tail call returning its arg must survive")
(assert (= (leaves-to-tail 1) 7)
        "tail call over destructured leaves must still run")
(assert (= (one-leaf-to-tail 1) 7)
        "tail call over one destructured leaf must still run")
(assert (= (alias-to-tail 1) 7)
        "tail call over a whole-value alias must still run")

# Boundedness: region growth must not scale with iteration count.
(let [d1 (region-delta pass-to-tail 200)
      d2 (region-delta pass-through-tail 200)
      d3 (region-delta keep-then-tail 200)
      d4 (region-delta leaves-to-tail 200)
      d5 (region-delta one-leaf-to-tail 200)
      d6 (region-delta alias-to-tail 200)]
  (assert (%lt d1 20) (concat "pass-to-tail leak: delta=" (number->string d1)))
  (assert (%lt d2 20)
          (concat "pass-through-tail leak: delta=" (number->string d2)))
  (assert (%lt d3 20) (concat "keep-then-tail leak: delta=" (number->string d3)))
  (assert (%lt d4 20) (concat "leaves-to-tail leak: delta=" (number->string d4)))
  (assert (%lt d5 20)
          (concat "one-leaf-to-tail leak: delta=" (number->string d5)))
  (assert (%lt d6 20) (concat "alias-to-tail leak: delta=" (number->string d6))))
