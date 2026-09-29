(elle/epoch 12)
# What a call costs in PAGES — the third heap dimension
# (docs/impl/region/model.md § "Page recycling"; docs/regions/performance.md
# § "A call into a variadic stdlib operator allocates").
#
# Regions never share pages (Rule 6), so a shape's page count is not its object
# count: three regions holding one object each own three pages. `arena/count`
# and `arena/region-count` cannot see that, and a shape can be perfectly
# leak-free by both and still claim a page on every call. `arena/page-claims`
# is the gauge for the page dimension — monotonic, never decremented on
# release, and Immediate, so sampling it allocates nothing and does not perturb
# the measurement.
#
# Every bound below is a CEILING, so the file is shrink-only: a shape that gets
# cheaper stays green, and only a shape that claims MORE pages per call than it
# does today goes red. Three groups:
#
#   - the free shapes (0 pages): an intrinsic, a fixed-arity call, and a
#     variadic call whose rest list is empty. These are exact — a ceiling of
#     zero admits nothing.
#   - the rest-list shapes: a variadic callee collects its rest arguments into
#     a cons chain, and every cons is born in its own region, which owns a
#     page. So the count is the number of rest arguments.
#   - the stdlib arithmetic wrappers, which are variadic Elle functions with a
#     `letrec` in the body: the rest conses plus the closure.
#
# The recycle side of the same contract — that claiming a page costs a
# free-list pop and no kernel call, that the blank-body debt is paid at release
# over the bytes the region wrote, and that a cached page keeps the stamp the
# stale-deref check reads — is pinned in Rust, by `pagepool::tests` and
# `regionpool::tests::teardown_blanks_the_page_body_and_keeps_the_header`.

(def window 4000)

(defn claims [thunk warm window]
  "Pages claimed over WINDOW calls of THUNK, after WARM untimed calls."
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/page-claims))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/page-claims) before))

# subjects ─────────────────────────────────────────────────────────────────────

(defn nop [x]
  "Fixed arity: the argument lands in an env slot, so nothing is collected."
  x)

(defn vnop [& xs]
  "Variadic: the arguments are collected into a rest list, one cons each."
  xs)

# the gauge-live discriminator ─────────────────────────────────────────────────
# A ceiling reads green against a DEAD gauge too, so measure a shape whose page
# cost cannot be zero first and require that it moves. `(pair 1 2)` mints a
# region for the cons, and a region owns a page. If this reads under one page
# per call the gauge is not measuring and every verdict below is void.

(def live-d (claims (fn [] (pair 1 2)) 200 window))
(println "region-page-recycle: discriminator (pair 1 2) claimed " live-d
         " pages over " window " calls")
(assert (%ge live-d window)
        (concat "arena/page-claims is dead: a cons per call claimed only "
                (number->string live-d) " pages over " (number->string window)
                " calls — every ceiling below is void"))

# measurements ─────────────────────────────────────────────────────────────────

(def empty-d (claims (fn [] nil) 200 window))
(def intrinsic-d (claims (fn [] (%add 1 2)) 200 window))
(def fixed-d (claims (fn [] (nop 1)) 200 window))
(def cmp-d (claims (fn [] (< 1 2)) 200 window))
(def rest1-d (claims (fn [] (vnop 1)) 200 window))
(def rest2-d (claims (fn [] (vnop 1 2)) 200 window))
(def add2-d (claims (fn [] (+ 1 2)) 200 window))
(def add3-d (claims (fn [] (+ 1 2 3)) 200 window))
(def mul2-d (claims (fn [] (* 2 3)) 200 window))
(def sub2-d (claims (fn [] (- 5 3)) 200 window))

(println "  free:      empty " empty-d "  %add " intrinsic-d "  fixed-arity "
         fixed-d "  (< a b) " cmp-d)
(println "  rest list: one arg " rest1-d "  two args " rest2-d)
(println "  wrappers:  (+ a b) " add2-d "  (+ a b c) " add3-d "  (* a b) "
         mul2-d "  (- a b) " sub2-d)

(defn at-most [d per-call label]
  (def ceiling (%mul per-call window))
  (assert (%le d ceiling)
          (concat label " claimed " (number->string d) " pages over "
                  (number->string window) " calls, over the ceiling of "
                  (number->string ceiling) " (" (number->string per-call)
                  " per call)")))

# free shapes: a ceiling of zero admits nothing.
(at-most empty-d 0 "an empty thunk")
(at-most intrinsic-d 0 "the %add intrinsic")
(at-most fixed-d 0 "a fixed-arity call")
(at-most cmp-d 0 "(< a b), whose rest list is empty at two arguments")

# rest-list shapes: one page per rest argument.
(at-most rest1-d 1 "a variadic call with one rest argument")
(at-most rest2-d 2 "a variadic call with two rest arguments")

# the arithmetic wrappers: the rest conses plus the body's letrec closure.
(at-most add2-d 3 "(+ a b)")
(at-most add3-d 4 "(+ a b c)")
(at-most mul2-d 3 "(* a b)")
(at-most sub2-d 2 "(- a b), whose first operand is a fixed parameter")

# The pages a loop claims are RECYCLED, not accumulated: the region dies at the
# end of its call, its page goes back to the per-thread cache, and the next call
# claims that same page. So a shape with a nonzero per-call page cost still runs
# in bounded memory — the claim count grows with the iteration count while the
# heap's byte footprint does not.
(defn bytes-growth [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/bytes))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/bytes) before))

(def growth-d (bytes-growth (fn [] (+ 1 2)) 200 window))
(println "  (+ a b) over " window " calls: " add2-d " page claims, " growth-d
         " bytes of heap growth")
(assert (%lt growth-d 65536)
        (concat "(+ a b) grew the heap by " (number->string growth-d)
                " bytes over " (number->string window)
                " calls — its pages are accumulating instead of recycling"))

(println "region-page-recycle: ok")
