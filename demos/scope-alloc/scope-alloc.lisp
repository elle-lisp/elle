(elle/epoch 6)

# Scope Allocation Workload — measuring escape analysis tiers
#
# Runs allocation-heavy loops in child fibers, comparing scoped (freed at
# scope exit) vs unscoped (freed at fiber death) patterns.
#
# Each tier of the escape analysis recognises a wider class of "safe"
# scopes that can release heap objects early. This demo quantifies the
# benefit by measuring arena/count (live objects) and arena/stats
# (region enters / destructors run) inside non-yielding child fibers.
#
# For the compile-time rejection breakdown, run with:
#   ELLE_SCOPE_STATS=1 elle demos/scope-alloc/scope-alloc.lisp

(defn run [thunk]
  "Execute thunk in a non-yielding child fiber."
  (fiber/resume (fiber/new thunk 1)))


# ── Tier 1: primitive whitelist ──────────────────────────────────────
#
# (length data) is whitelisted as returning an immediate, so a let that
# allocates temp data and returns (length ...) qualifies for scope alloc.

(println "tier 1 — primitive whitelist (length)")

(var tier1-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ([data @[1 2 3 4 5]])
        (length data))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(var tier1-unscoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (var last nil)
    (while (< i 10000)
      (let ([data @[1 2 3 4 5]])
        (assign last data))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(println "  scoped:   " tier1-scoped " live objects after 10k iters")
(println "  unscoped: " tier1-unscoped " live objects after 10k iters")
(println "  saved:    " (- tier1-unscoped tier1-scoped) " objects freed early")
(println)


# ── Tier 3: returning an outer binding ───────────────────────────────
#
# When the let body returns a variable from *outside* the let, the
# result is safe (allocated before the scope's RegionEnter).

(println "tier 3 — returning outer binding")

(var tier3-stats
  (run (fn []
    (var i 0)
    (var outer-val 0)
    (while (< i 10000)
      (let ([temp @[1 2 3]])
        (assign outer-val (+ outer-val (length temp)))
        outer-val)
      (assign i (+ i 1)))
    (arena/stats))))

(println "  enters:    " tier3-stats:enters)
(println "  dtors-run: " tier3-stats:dtors-run)
(println)


# ── Tier 4: nested lets ─────────────────────────────────────────────
#
# Both the outer and inner let qualify. The inner let's bindings are
# part of the outer scope's region; result_is_safe recurses through.

(println "tier 4 — nested lets reducing to arithmetic")

(var tier4-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ([xs @[10 20 30]])
        (let ([n (length xs)])
          (+ n 1)))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(println "  scoped net: " tier4-scoped " live objects after 10k iters")
(println)


# ── Tier 5: match returning immediates ──────────────────────────────
#
# All match arms return keywords or ints → result is safe.

(println "tier 5 — match arms returning keywords")

(var tier5-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ([tag (mod i 3)])
        (match tag
          (0 :zero)
          (1 :one)
          (_ :other)))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(println "  scoped net: " tier5-scoped " live objects after 10k iters")
(println)


# ── Tier 8: immediate outward set ───────────────────────────────────
#
# (assign counter (+ counter 1)) writes an immediate outward. Tier 8
# recognises that an outward set with a provably immediate value is
# harmless, so the while's implicit block scope-allocates.

(println "tier 8 — immediate outward set in while")

(var tier8-stats
  (run (fn []
    (var counter 0)
    (while (< counter 10000)
      (let ([tmp @[1 2 3]])
        (length tmp))
      (assign counter (+ counter 1)))
    (arena/stats))))

(println "  enters:    " tier8-stats:enters
  " (10000 inner let + 1 while block = " tier8-stats:enters ")")
(println "  dtors-run: " tier8-stats:dtors-run)
(println)


# ── Combined workload ────────────────────────────────────────────────
#
# A single fiber exercising all tiers in a tight loop.

(println "combined — all tiers in one fiber")

(var combined
  (run (fn []
    (var before (arena/count))
    (var total 0)
    (var i 0)
    (while (< i 5000)
      # Tier 1: whitelist (length)
      (let ([xs @[1 2 3 4 5]])
        (assign total (+ total (length xs))))
      # Tier 3: return outer binding
      (let ([tmp @[10 20]])
        total)
      # Tier 4: nested let → arithmetic
      (let ([a @[1 2]])
        (let ([n (length a)])
          (+ n i)))
      # Tier 5: match → keyword
      (let ([tag (mod i 2)])
        (match tag (0 :even) (_ :odd)))
      (assign i (+ i 1)))
    {:net (- (arena/count) before)
     :total total
     :stats (arena/stats)})))

(println "  net objects: " combined:net)
(println "  total sum:   " combined:total)
(println "  scope stats: " combined:stats)
(println)
(println "done.")
