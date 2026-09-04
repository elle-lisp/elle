(elle/epoch 12)
# A collector parameter's collected value is released once per call, whoever
# calls it.
#
# `&`, `&keys` and `&named` all collect their surplus arguments into ONE value
# built by the callee's calling convention in a region of its own
# (`collect_struct_in_own_region` / `args_to_list`, src/vm/env.rs). One
# reference stands on that region, and the callee owes it a release at the
# collector parameter's last use.
#
# ── Why the call site is at file scope ──────────────────────────────
#
# The region walk INLINES a callee it can resolve to a lambda, to collect the
# edges of the intrinsics inside it. Which call sites resolve that way is not a
# property a reader can see from the callee, so each drive below is written at
# file scope, where the resolution holds — a drive wrapped in a `defn` does not
# inline here, reads 0 whatever the callee's parameter list does, and would pass
# on a compiler that never releases a collected value at all.
#
# ── Reading the numbers ─────────────────────────────────────────────
#
# Each drive runs the same loop at `calls` iterations after an identical warmup
# loop, so a per-call cost reads `calls` and a one-off reads about 0. The
# controls are the same drive over a callee with a plain parameter list: they
# fix the window's own bookkeeping cost, which is what `slack` is sized for. A
# per-call leak is two orders of magnitude above it.

(def calls 200)
(def slack 20)

(defn plain [a]
  (if a 1 0))
(defn rest-list [& xs]
  (length xs))
(defn keys-struct [&keys k]
  (if k 1 0))
(defn named-struct [&named a b]
  (if a 1 0))

# ── plain control ────────────────────────────────────────────────────

(def @i 0)
(while (< i calls)
  (plain 1)
  (assign i (+ i 1)))
(def plain-objects0 (arena/count))
(def plain-regions0 (arena/region-count))
(assign i 0)
(while (< i calls)
  (plain 1)
  (assign i (+ i 1)))
(def plain-objects (- (arena/count) plain-objects0))
(def plain-regions (- (arena/region-count) plain-regions0))

# ── (& xs) ───────────────────────────────────────────────────────────

(assign i 0)
(while (< i calls)
  (rest-list 1 2 3)
  (assign i (+ i 1)))
(def rest-objects0 (arena/count))
(def rest-regions0 (arena/region-count))
(assign i 0)
(while (< i calls)
  (rest-list 1 2 3)
  (assign i (+ i 1)))
(def rest-objects (- (arena/count) rest-objects0))
(def rest-regions (- (arena/region-count) rest-regions0))

# ── (&keys k) ────────────────────────────────────────────────────────

(assign i 0)
(while (< i calls)
  (keys-struct :a 1 :b 2)
  (assign i (+ i 1)))
(def keys-objects0 (arena/count))
(def keys-regions0 (arena/region-count))
(assign i 0)
(while (< i calls)
  (keys-struct :a 1 :b 2)
  (assign i (+ i 1)))
(def keys-objects (- (arena/count) keys-objects0))
(def keys-regions (- (arena/region-count) keys-regions0))

# ── (&named a b) ─────────────────────────────────────────────────────

(assign i 0)
(while (< i calls)
  (named-struct :a 1)
  (assign i (+ i 1)))
(def named-objects0 (arena/count))
(def named-regions0 (arena/region-count))
(assign i 0)
(while (< i calls)
  (named-struct :a 1)
  (assign i (+ i 1)))
(def named-objects (- (arena/count) named-objects0))
(def named-regions (- (arena/region-count) named-regions0))

# ── report and assert ────────────────────────────────────────────────

(println "region-inline-rest-param-leak over " calls
         " calls (object/region deltas):")
(println "  plain  obj=" plain-objects " reg=" plain-regions)
(println "  &      obj=" rest-objects " reg=" rest-regions)
(println "  &keys  obj=" keys-objects " reg=" keys-regions)
(println "  &named obj=" named-objects " reg=" named-regions)

(assert (<= plain-objects slack)
        (string "control: a plain call leaks objects, delta=" plain-objects))
(assert (<= plain-regions slack)
        (string "control: a plain call leaks regions, delta=" plain-regions))

(assert (<= rest-objects slack)
        (string "(& xs) leaks objects per call, delta=" rest-objects))
(assert (<= rest-regions slack)
        (string "(& xs) leaks regions per call, delta=" rest-regions))

(assert (<= keys-objects slack)
        (string "(&keys k) leaks objects per call, delta=" keys-objects))
(assert (<= keys-regions slack)
        (string "(&keys k) leaks regions per call, delta=" keys-regions))

(assert (<= named-objects slack)
        (string "(&named a b) leaks objects per call, delta=" named-objects))
(assert (<= named-regions slack)
        (string "(&named a b) leaks regions per call, delta=" named-regions))

(println "region-inline-rest-param-leak: ok")
