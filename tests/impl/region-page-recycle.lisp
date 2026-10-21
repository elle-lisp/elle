(elle/epoch 15)
# audited: 2026-10-20
# What a call costs in PAGES, the heap dimension the object and region gauges cannot see.
# docs/impl/region/model.md
# docs/regions/performance.md
# docs/impl/region/restlist.md
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
#     comparison wrapper whose two-operand arm the compiler grafts.
#   - the rest-list shapes: a variadic callee that takes no tail of its rest
#     list keeps the whole list in one region, so it costs one page whatever its
#     length.
#   - the stdlib arithmetic wrappers: a two-operand call grafts and claims
#     nothing, and a call the graft declines builds a rest list its `letrec`
#     walker may keep a tail of — one page per cons — plus the closure.
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

# the rest-list shapes: one page for the whole list ────────────────────────────
(page-rate "(vnop 1)" (fn [j] (vnop 1)))
(page-rate "(vnop 1 2)" (fn [j] (vnop 1 2)))

# the arithmetic wrappers: grafted, or the rest conses plus the closure ───────
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
