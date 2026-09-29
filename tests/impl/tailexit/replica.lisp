(elle/epoch 12)
# audited: 2026-09-08
# The witnesses a REPLICA reaches: a branch arm, a forward cell no binding names, and a region an argument only used.
#
# docs/impl/region/replicate.md
# ── the per-arm face: a branch merge inherits its arms' relocation points ─────
# A release emitted past the merge is replicated ahead of each arm's `TailCall`
# and still emitted at the merge, so both obligations are re-asked per point and
# a third appears: the two copies must act exactly ONCE on a path that reaches
# both.

# (i) the argument is moved into ONE arm's tail call and read there. The
# exemption is per point, so that arm must keep its release in the dead block
# even though its sibling is free to take a copy.
(defn i-arm-moved (v t)
  (if t (a-callee v) 0))
(defn i-arm (i)
  (i-arm-moved (list (string "i" i) i) true))

# (j) the arm's tail callee reaches the value through its CAPTURED environment —
# the admission, now asked of a replica rather than of an in-block move.
(defn j-arm-captured (v t)
  (let [g (fn () (length (first v)))]
    (if t (g) 0)))
(defn j-arm (i)
  (j-arm-captured (list (string "j" i) i) true))

# (k) the arm's tail call is to a NATIVE, which pushes no frame and falls
# through to the merge — so this path reaches the replica AND the merge copy.
# Acting twice drops the reference the CALLER still holds, freeing `x` under its
# read. This is the row the self-cancelling requirement exists for.
(defn k-arm-native (v w t)
  (if t (length w) 0))
(defn k-arm (i)
  (let [x (list (string "k" i) i)]
    (k-arm-native x (string "w" i) true)
    (length (first x))))

# ── the short-circuit face: an arm the source spells as one expression ────────
# `and` and `or` carry no branch in the source and the lowerer gives them one:
# each operand stores its value into the result slot, and every operand but the
# last branches on that value — to the done block where the answer is settled, to
# the next operand otherwise. So the done block is a merge reached through every
# operand's block, and the operands are its arms
# (docs/impl/region/replicate.md § "A short-circuit operand is an arm"). Only the
# LAST operand inherits tail position, so it is the only arm that can carry a
# frame-replacing tail call, and the replica the merge sends into it owes exactly
# what the rows above owe — re-asked here because the arms are found by a
# different route.

(defn sc-read-len (a)
  (length a:k))

# (sc1) the arm MOVES `x` into its callee, whose owned-param release is what frees
# it. A replica ahead of the call drops that reference first, and the callee's
# read of `a:k` finds a freed page.
(defn sc-moved-inner (x t)
  (or t (sc-read-len x)))
(defn sc-moved (i)
  (let [r (sc-moved-inner {:k (string "m" i "-long")} false)]
    (if r 1 0)))

# (sc2) the arm's callee reaches `x` through its CAPTURED environment, which no
# argument names. The funnel counted that hold when the env was built, so the
# replica must leave the callee's read standing.
(defn sc-captured-inner (x t)
  (let [g (fn () (length x:k))]
    (or t (g))))
(defn sc-captured (i)
  (let [r (sc-captured-inner {:k (string "c" i "-long")} false)]
    (if r 1 0)))

# (sc3) the arm's callee hands `x` BACK, so the caller's reference is minted by the
# callee's own `Return` — after the relocated release has run. The counted env
# edge is what holds the region across that gap.
(defn sc-handback-inner (x t)
  (let [g (fn () x)]
    (or t (g))))
(defn sc-handback (i)
  (let [r (sc-handback-inner {:k (string "h" i "-long")} false)]
    (length r:k)))

# (sc4) `x` escapes into a container that outlives the frame before the branch, and
# is read back out afterwards. The store's incref is what the replica must not
# take below zero.
(defn sc-store-inner (x t)
  (push sink x)
  (or t (tail-sink)))
(defn sc-store (i)
  (sc-store-inner {:k (string "s" i "-long")} false)
  (length (get (get sink (%sub (length sink) 1)) :k)))

# (sc5) a closure that ESCAPES captured `x`; calling it after the frame is gone must
# still reach the captured region.
(defn sc-escape-inner (x t)
  (let [g (fn () (length x:k))]
    (push sink g)
    (or t (tail-sink))))
(defn sc-escape (i)
  (sc-escape-inner {:k (string "e" i "-long")} false)
  (let [g (get sink (%sub (length sink) 1))]
    (if (g) 1 0)))

# (sc6) the arm's callee is a NATIVE, which pushes no frame — so the fall-through
# runs the replica AND reaches the merge, where the same release stands. Two
# copies on one path count once only because the value route nil-stamps the slot
# it read; without the stamp the second decref names a freed and recycled region.
(defn sc-native-inner (x t)
  (or t (length "abcdef")))
(defn sc-native (i)
  (let [x {:k (string "n" i "-long")}]
    (sc-native-inner x false)
    (length x:k)))

# ── the forward-cell face: a region no binding names ──────────────────────────
# A prebound FORWARD CELL is reached through its binding's verdict rather than its
# own, because a binding names the closure region its cell points AT (see
# docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
# its binding is"). The relocated release drops the frame's slot reference; what
# must stand is the counted `closure ⊇ cell` edge the capturer's env took, since
# every later `DerefCell` that dispatches the sibling resolves through the cell.

# (l) the sibling is reached through the cell on every step of the recursion, and
# reads a heap value of its own. The cell's release now fires ahead of the letrec
# body's tail call, so a release that reached zero recycles the cell under the very
# read that dispatches `helper`.
(defn l-fwd (i)
  (let [src (list (string "l" i) i)]
    (letrec [helper (fn (k) (%add k (length (first src))))
             go (fn (k) (helper k))]
      (go 0))))
(defn l-read (i)
  (l-fwd i))

# (m) the CAPTURER is handed back, so what stands between the relocated release and
# the caller's minted reference is the callee's `closure ⊇ cell` edge. The caller
# drives the returned closure afterwards, and every step of that drive derefs the
# cell and reads `src` through the sibling, all after the defining frame is gone.
(defn m2-fwd (i)
  (let [src (list (string "m" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1)
                    go
                    (begin
                      (helper k)
                      (go (%sub k 1)))))]
      (go 3))))
(defn m2-read (i)
  (let [g (m2-fwd i)]
    (if (nil? (g 3)) 0 1)))

# (n) the cell's own CONTENT is handed back: `go` returns `helper`, so the sibling
# outlives the cell that held it and the caller calls it directly. The cell's
# relocated release must leave the sibling's region standing on the caller's mint.
(defn n2-fwd (i)
  (let [src (list (string "n" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1) helper (go (%sub k 1))))]
      (go 2))))
(defn n2-read (i)
  ((n2-fwd i) 0))

# (s) the SIBLING captures the self-recursive member and the letrec body tail-calls
# the sibling, so ONE tail call carries both deferred channels — the merged arena's
# `deferred_release_slot` and the sibling's own `defer_callee_release`
# (docs/impl/region/letrec.md § "The arena channel and the callee channel are
# independent"). Each drops a different frame reference; running either twice takes
# a live region to zero and recycles its pages under the next call's walk. `go`
# reads a heap value of its own on every step, and reaches it through the cell the
# arena holds.
(defn s-sib (i)
  (let [src (list (string "s" i) i)]
    (letrec [go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1) (length (first src)) (go (%sub k 1))))
             outer (fn (k)
                     (when (%not (%int? k)) (error :k))
                     (go k))]
      (outer 3))))
(defn s-read (i)
  (s-sib i))

# (t) the letrec body tail-calls the CAPTURED sibling rather than the capturer, so
# the callee's own region is the one the relocation must leave alone and the
# deferral is what runs its release — at the callee's normal completion, never
# before the frame it replaces has run (docs/impl/region/mechanism.md § "What the
# exemption keeps, a channel must still run"). `helper` reads a heap value through
# its OWN environment on the far side of the frame replacement, so a release that
# fired ahead of the `TailCall` recycles the pages that read resolves through, and
# one that ran twice recycles them under the caller.
(defn t-member (i)
  (let [src (list (string "t" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k) (helper k))]
      (helper (go 0)))))
(defn t-read (i)
  (t-member i))

# ── the operand-value face: what an argument's evaluation merely USED ─────────
# The exemption reads an operand's VALUE, not its syntax (docs/impl/region/mechanism.md
# § "What an operand names is its VALUE, not its syntax"), so a region reached only
# inside an argument's own nested call is no longer exempt and its release does fire
# ahead of the `TailCall`. Three ways the value can still live in such a region, each
# with a different reference standing between the relocated release and the callee's
# read.

# (o) the argument's nested call HANDS BACK one of its own arguments, so the value
# moved into the tail call lives in the region the release just dropped. The
# callee's `Return` minted for it, and that mint is what must stand.
(defn o-ident (v)
  v)
(defn o-passthrough (v)
  (a-callee (o-ident v)))
(defn o-read (i)
  (o-passthrough (list (string "o1" i) i)))

# (p) the argument's nested call is a native container READ, whose result is a
# borrow living INSIDE the container. The pass-through retain the native took is
# the reference the relocated release must leave standing.
(defn p-callee (s)
  (%add 1 (length s)))
(defn p-read-elem (v)
  (p-callee (first v)))
(defn p-read (i)
  (p-read-elem (list (string "p1" i) i)))

# (q) the argument is an inline `%`-opcode read — which mints no region, so its
# result lives in the operand's region with no reference of its own. That is why the
# container's own release is extended to the reader (Rule 4) and lands in the dead
# block: the operand is the value-producing leaf and stays exempt, because hoisting
# the container's release would free the page the callee is handed.
(defn q-read (i)
  (let [v (%pair (string "q1" i) nil)]
    (p-callee (%first v))))

# (r) the argument is a fresh LAMBDA capturing a local, so the closure region is
# the moved value and stays exempt while the capture's own counted edge is what
# keeps the captured value alive under the callee's call.
(defn r-call-thunk (g)
  (length (first (g))))
(defn r-lambda-arg (i)
  (let [x (list (string "r1" i) i)]
    (r-call-thunk (fn () x))))
