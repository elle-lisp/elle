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

# Boundedness: region growth must not scale with iteration count.
(let [d1 (region-delta pass-to-tail 200)
      d2 (region-delta pass-through-tail 200)
      d3 (region-delta keep-then-tail 200)]
  (assert (%lt d1 20) (concat "pass-to-tail leak: delta=" (number->string d1)))
  (assert (%lt d2 20)
          (concat "pass-through-tail leak: delta=" (number->string d2)))
  (assert (%lt d3 20) (concat "keep-then-tail leak: delta=" (number->string d3))))
