(elle/epoch 14)
# audited: 2026-10-05
# What a call costs in PAGES — the third heap dimension
# (docs/impl/region/model.md § "Page recycling"; docs/regions/performance.md
# § "A call into a variadic stdlib operator allocates").
#
# Regions never share pages (Rule 6), so a shape's page count is not its object
# count: three regions holding one object each own three pages. `arena/count`
# and `arena/region-count` cannot see that, and a shape can be perfectly
# leak-free by both and still claim a page on every call. `arena/page-claims`
# is the gauge for the page dimension — monotonic, never decremented on
# release — and the ratchet reads it as a rate per call against the rows of
# tests/ledger/region-page-recycle.lisp. Three groups:
#
#   - the free shapes (0 pages): an intrinsic, a fixed-arity call, and a
#     variadic call whose rest list is empty.
#   - the rest-list shapes: a variadic callee collects its rest arguments into
#     a cons chain, and every cons is born in its own region, which owns a
#     page. So the count is the number of rest arguments.
#   - the stdlib arithmetic wrappers, which are variadic Elle functions with a
#     `letrec` in the body: the rest conses plus the closure.
#
# The operands are literals, as ordinary code writes them; each probe takes the
# op's index and leaves it unread.
#
# The recycle side of the same contract — that claiming a page costs a
# free-list pop and no kernel call, that the blank-body debt is paid at release
# over the bytes the region wrote, and that a cached page keeps the stamp the
# stale-deref check reads — is pinned in Rust, by `pagepool::tests` and
# `regionpool::tests::teardown_blanks_the_page_body_and_keeps_the_header`.

(def r ((import "std/ratchet")))

(defn page-rate [subject probe]
  (r:rate subject probe :on [r:pages] :block 400 :min 6))

# subjects ─────────────────────────────────────────────────────────────────────

(defn nop [x]
  "Fixed arity: the argument lands in an env slot, so nothing is collected."
  x)

(defn vnop [& xs]
  "Variadic: the arguments are collected into a rest list, one cons each."
  xs)

# the free shapes ──────────────────────────────────────────────────────────────
(page-rate "empty thunk" (fn [j] nil))
(page-rate "%add" (fn [j] (%add 1 2)))
(page-rate "fixed-arity call" (fn [j] (nop 1)))
(page-rate "(< a b)" (fn [j] (< 1 2)))

# the rest-list shapes: one page per rest argument ─────────────────────────────
(page-rate "(vnop 1)" (fn [j] (vnop 1)))
(page-rate "(vnop 1 2)" (fn [j] (vnop 1 2)))

# the arithmetic wrappers: the rest conses plus the body's letrec closure ──────
(page-rate "(+ a b)" (fn [j] (+ 1 2)))
(page-rate "(+ a b c)" (fn [j] (+ 1 2 3)))
(page-rate "(* a b)" (fn [j] (* 2 3)))
(page-rate "(- a b)" (fn [j] (- 5 3)))

# The pages a loop claims are RECYCLED, not accumulated: the region dies at the
# end of its call, its page goes back to the per-thread cache, and the next call
# claims that same page. So a shape with a nonzero per-call page cost still runs
# in bounded memory — the claim count grows with the iteration count while the
# heap's byte footprint does not.
(r:rate "(+ a b)" (fn [j] (+ 1 2)) :on [r:bytes] :block 400 :min 6)

(println "region-page-recycle: ok")
