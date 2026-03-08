# Scope Allocation Workload — measuring escape analysis tiers
#
# Runs allocation-heavy loops in child fibers, comparing scoped (freed at
# scope exit) vs unscoped (freed at fiber death) patterns.
#
# Each tier of the escape analysis recognises a wider class of "safe"
# scopes that can release heap objects early. This demo quantifies the
# benefit by measuring arena/count (live objects) and arena/scope-stats
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

(display "tier 1 — primitive whitelist (length)\n")

(var tier1-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ((data @[1 2 3 4 5]))
        (length data))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(var tier1-unscoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (var last nil)
    (while (< i 10000)
      (let ((data @[1 2 3 4 5]))
        (assign last data))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(display "  scoped:   ") (display tier1-scoped) (print " live objects after 10k iters")
(display "  unscoped: ") (display tier1-unscoped) (print " live objects after 10k iters")
(display "  saved:    ") (display (- tier1-unscoped tier1-scoped)) (print " objects freed early")
(print "")


# ── Tier 3: returning an outer binding ───────────────────────────────
#
# When the let body returns a variable from *outside* the let, the
# result is safe (allocated before the scope's RegionEnter).

(display "tier 3 — returning outer binding\n")

(var tier3-stats
  (run (fn []
    (var i 0)
    (var outer-val 0)
    (while (< i 10000)
      (let ((temp @[1 2 3]))
        (assign outer-val (+ outer-val (length temp)))
        outer-val)
      (assign i (+ i 1)))
    (arena/scope-stats))))

(display "  enters:    ") (display (get tier3-stats :enters)) (print "")
(display "  dtors-run: ") (display (get tier3-stats :dtors-run)) (print "")
(print "")


# ── Tier 4: nested lets ─────────────────────────────────────────────
#
# Both the outer and inner let qualify. The inner let's bindings are
# part of the outer scope's region; result_is_safe recurses through.

(display "tier 4 — nested lets reducing to arithmetic\n")

(var tier4-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ((xs @[10 20 30]))
        (let ((n (length xs)))
          (+ n 1)))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(display "  scoped net: ") (display tier4-scoped) (print " live objects after 10k iters")
(print "")


# ── Tier 5: match returning immediates ──────────────────────────────
#
# All match arms return keywords or ints → result is safe.

(display "tier 5 — match arms returning keywords\n")

(var tier5-scoped
  (run (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 10000)
      (let ((tag (mod i 3)))
        (match tag
          (0 :zero)
          (1 :one)
          (_ :other)))
      (assign i (+ i 1)))
    (- (arena/count) before))))

(display "  scoped net: ") (display tier5-scoped) (print " live objects after 10k iters")
(print "")


# ── Tier 8: immediate outward set ───────────────────────────────────
#
# (assign counter (+ counter 1)) writes an immediate outward. Tier 8
# recognises that an outward set with a provably immediate value is
# harmless, so the while's implicit block scope-allocates.

(display "tier 8 — immediate outward set in while\n")

(var tier8-stats
  (run (fn []
    (var counter 0)
    (while (< counter 10000)
      (let ((tmp @[1 2 3]))
        (length tmp))
      (assign counter (+ counter 1)))
    (arena/scope-stats))))

(display "  enters:    ") (display (get tier8-stats :enters))
(display "  (10000 inner let + 1 while block = ")
(display (get tier8-stats :enters)) (print ")")
(display "  dtors-run: ") (display (get tier8-stats :dtors-run)) (print "")
(print "")


# ── Combined workload ────────────────────────────────────────────────
#
# A single fiber exercising all tiers in a tight loop.

(display "combined — all tiers in one fiber\n")

(var combined
  (run (fn []
    (var before (arena/count))
    (var total 0)
    (var i 0)
    (while (< i 5000)
      # Tier 1: whitelist (length)
      (let ((xs @[1 2 3 4 5]))
        (assign total (+ total (length xs))))
      # Tier 3: return outer binding
      (let ((tmp @[10 20]))
        total)
      # Tier 4: nested let → arithmetic
      (let ((a @[1 2]))
        (let ((n (length a)))
          (+ n i)))
      # Tier 5: match → keyword
      (let ((tag (mod i 2)))
        (match tag (0 :even) (_ :odd)))
      (assign i (+ i 1)))
    {:net (- (arena/count) before)
     :total total
     :stats (arena/scope-stats)})))

(display "  net objects: ") (display (get combined :net)) (print "")
(display "  total sum:   ") (display (get combined :total)) (print "")
(display "  scope stats: ") (print (get combined :stats))
(print "")
(print "done.")
