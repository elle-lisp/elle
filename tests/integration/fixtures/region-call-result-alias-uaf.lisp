(elle/epoch 12)
# tests/integration/fixtures/region-call-result-alias-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression SIGSEGVs under
# --trace=guardfree (and panics on the generation stamp under the plain VM), and
# `make smoke` globs tests/elle/*.lisp into one shared process where a segfault
# would take the whole harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_call_result_alias_uaf`).
#
# WHAT IT GUARDS — an opaque CALL's result is not provably distinct from its
# arguments. `(push a s)` funnel-adopts `s`'s region into `a`'s Owned subtree, which
# FREEZES `s`'s reference count: from then on the only thing that reclaims `s` is
# `a`'s own release, and any Rule 5 pass-through retain taken over `s` is inert. That
# is sound exactly as long as the forest can bound every alias that names `s` — which it
# does for a native read whose container argument IS `a` — see adopt.md, § "The
# lifetime obligation the root carries". A CALL hides the container, in two ways:
#
#   - THE RESULT IS THE CONTAINER. `(concat a @[1 2])` on a MUTABLE first argument
#     extends `a` in place and returns `a` itself, so the caller's call-result
#     placeholder resolves at runtime to `a`'s own region. A native `first`/`get` read
#     out of that placeholder is a read out of `a` — but its recorded container is the
#     placeholder, which relates to no member, so the member obligation never sees it.
#     `a`'s release then subtree-drops the element the reader still holds.
#
#   - THE RESULT IS A MEMBER. `(last a)` hands back a value it read out of `a` — the
#     adopted element itself. The caller's placeholder names a frozen member directly,
#     and the reader's own value-resolved release faults on the freed page.
#
# Only a declaration that the heap result lives in the call's OWN minted region rules
# either out — `Fresh`, `Stores`, `Sends` (the claim the effects oracle checks on every
# debug run) — or `Immediate`, which returns no region. Everything else, a user/stdlib
# callee included, must be treated as handing back something inside an argument.
#
# DISTINCT FROM `region-container-read-borrow-uaf`, where the read's container argument
# is the container itself and the walk sees it directly; this fixture is the face where
# a call stands between them.

# The result-is-the-container face: concat on a mutable first argument returns it, and
# the element read back out of the result outlives the whole expression.
(defn concat-read-first [i]
  (let [a (@array)]
    (push a (string "y" i))
    (length (first (concat a @[1 2])))))

# The same through `get`, and with an IMMUTABLE second argument — the aliasing is a
# property of the first argument's mutability, not of what it is concatenated with.
(defn concat-read-get [i]
  (let [a (@array)]
    (push a (string "y" i))
    (length (get (concat a [1 2]) 0))))

# The result-is-a-member face: `last` returns a value read out of its argument, so the
# caller's placeholder names the adopted element directly — no second read involved.
(defn last-of-container [i]
  (let [a (@array)]
    (push a (string "y" i))
    (length (last a))))

# The FUNNEL face. A `Funnel` declares its result to be arg0 in place or a fresh copy of
# it, so the result needs no bound of its own — it carries arg0's counted pass-through
# reference, and the trailing discarded store result of every mutable-store builder would
# otherwise refuse its own subtree. What it does carry is reachability: the inner store
# below returns `a` itself, so the `get` reads out of `a` and its alias names the frozen
# element, even though the container the read records is the funnel's placeholder.
(defn funnel-result-read [i]
  (let [a (@struct)]
    (%put a :k (string "y" i))
    (length (get (%put a :j 5) :k))))

# The same over an array, through the raw push funnel and `first`.
(defn funnel-push-read [i]
  (let [a (@array)]
    (%array-push a (string "y" i))
    (length (first (%array-push a 5)))))

# The CONTROL: the container is mentioned again after the call, so its release already
# followed the alias. Green before and after — it isolates the alias from the shape.
(defn call-then-reuse [i]
  (let [a (@array)]
    (push a (string "y" i))
    (let [n (length (first (concat a @[1 2])))]
      (+ n (length a)))))

# Drive one shape n times and return its steady-state region growth over those n ops,
# sampled by the program after a priming pass — the same gauge tests/elle/oracle.lisp
# reads, and the only trustworthy one (docs/impl/region/diagnostics.md § Diagnostics).
# Returned as a total rather than a per-op rate so the thresholds below carry an explicit
# slack, which a rounded rate would hide.
(defn growth-of [f n]
  (def @p 0)
  (while (< p n)
    (f p)
    (assign p (+ p 1)))
  (def r0 (arena/region-count))
  (def @i 0)
  (while (< i n)
    (f i)
    (assign i (+ i 1)))
  (- (arena/region-count) r0))

# The UAF face is the primary guard: a regression SIGSEGVs under --trace=guardfree
# somewhere in these 3000 ops, whatever the growths below say. The priming pass inside
# `growth-of` is what recycles freed pages onto live regions, so an over-free lands on a
# page some other region already owns.
(def first-growth (growth-of concat-read-first 500))
(def get-growth (growth-of concat-read-get 500))
(def last-growth (growth-of last-of-container 500))
(def funnel-growth (growth-of funnel-result-read 500))
(def push-growth (growth-of funnel-push-read 500))
(def reuse-growth (growth-of call-then-reuse 500))

# `last` hands back the adopted element itself, and refusing the adopt puts it back on
# the RC baseline — which reclaims it exactly. Bounded, both sides: this is the assertion
# that the refusal did not trade the over-free for a leak. The two funnel shapes reclaim
# on the same baseline and read bounded too. The slack is the sibling borrow fixture's:
# well under one region per op, so any per-op strand is caught while sampling jitter is
# not.
(defn pin-bounded [name g]
  (assert (< g 50) (string name " leaked " g " regions over 500 ops")))

(pin-bounded "the member an opaque call returned" last-growth)
(pin-bounded "the funnel-result read" funnel-growth)
(pin-bounded "the funnel-push read" push-growth)

# The concat shapes carry a separate, per-call residue: extending a MUTABLE first
# argument in place strands one region per call whether or not anything is ever read back
# out of the result — the growth a read-free concat measures, and the growth the control
# above measures without ever having faulted. It is not this mechanism's, so it is pinned
# SHRINK-ONLY here rather than asserted away: a fix lowers these, never raises them. One
# per op is the measured rate; the threshold is that plus the same slack.
(defn pin-shrink-only [name g]
  (assert (< g 550)
          (string name " grew to " g
                  " regions over 500 ops — this pin is shrink-only")))

(pin-shrink-only "concat-read-first" first-growth)
(pin-shrink-only "concat-read-get" get-growth)
(pin-shrink-only "call-then-reuse" reuse-growth)

(println "region-call-result-alias-uaf: ok")
