(elle/epoch 12)
# The runtime Syntax→Value materializer `Syntax::to_value` is an ordinary mortal
# allocation, so the read-time primitives that use it — `(read s)`, `(read-all
# s)`, `(syntax->datum stx)` — build their whole parsed structure into a
# reclaimable region and reclaim it at the caller's decref_point.
#
# These primitives are native-fns: `dispatch_native_call` mints a fresh runtime
# region per call and routes the primitive's allocations into it, then the fresh
# result is reclaimed at the caller's decref_point (the same machinery
# `region-native-result-leak.lisp` pins for `(string "x" "y")`). The whole parsed
# tree shares one reclaimable region, the result is reclaimable, and repeated
# calls stay bounded.
#
# This is the read/meta sibling of region-eval-quoted-data-leak.lisp.

# ── (a) attribution witness ───────────────────────────────────────
# The value a read-time primitive returns is an ordinary allocation in a real
# reclaimable region (id >= 2), not an immediate (id 0).
(def reg-read (arena/region-of (read "(a b c)")))
(assert (%ge reg-read 2)
        (concat "(read \"(a b c)\") must be an ordinary allocation in a real "
                "reclaimable region — got " (number->string reg-read)))

# a parsed STRING literal (string bytes laid out inline in the call's region)
(def reg-str (arena/region-of (read "\"hello-read\"")))
(assert (%ge reg-str 2)
        (concat "(read string) must be a real reclaimable region — got "
                (number->string reg-str)))

# read-all wraps the form list with a mortal spine already, and each parsed form
# (here the inner list `(a b c)`) is itself reclaimable.
(def reg-all (arena/region-of (first (read-all "(a b c) (d e)"))))
(assert (%ge reg-all 2)
        (concat "(read-all) inner form must be a real reclaimable region — got "
                (number->string reg-all)))

# syntax->datum runs the same materializer over a syntax object.
(def stx (datum->syntax nil (quote (x y z))))
(def reg-datum (arena/region-of (syntax->datum stx)))
(assert (%ge reg-datum 2)
        (concat "(syntax->datum) must be a real reclaimable region — got "
                (number->string reg-datum)))

# ── (b) value correctness ─────────────────────────────────────────
# Materializing into a mortal region must reproduce the parsed datum exactly.
(assert (= (read "(+ 1 2)") (quote (+ 1 2)))
        "(read) materializes the wrong list")
(assert (= (read "42") 42) "(read) materializes the wrong int")
(assert (= (read "\"hi\"") "hi") "(read) materializes the wrong string")
(assert (= (read-all "1 2 3") (quote (1 2 3)))
        "(read-all) materializes the wrong list")
(assert (= (syntax->datum (datum->syntax nil (quote (x y)))) (quote (x y)))
        "(syntax->datum) materializes the wrong value")

# ── (c) object-count leak witness ─────────────────────────────────
# Warm one-time effects, then measure per-iteration growth over a fixed window.
# A bounded read leaves the count flat; a per-call leak would grow ~5 objects/iter
# (five cons cells per `(a b c d e)`).
(defn churn-read (n)
  (var i 0)
  (while (%lt i n)
    (read "(a b c d e)")
    (assign i (%add i 1))))

(churn-read 200)
(def c-before (arena/count))
(churn-read 2000)
(def c-delta (%sub (arena/count) c-before))
(println "region-read-meta-leak: read object delta over 2000 reads = " c-delta)
(assert (%lt c-delta 100)
        (concat "(read) leaks objects, delta=" (number->string c-delta)))

# syntax->datum sibling: same materializer, same bound.
(defn churn-datum (n)
  (def s (datum->syntax nil (quote (a b c d e))))
  (var i 0)
  (while (%lt i n)
    (syntax->datum s)
    (assign i (%add i 1))))

(churn-datum 200)
(def d-before (arena/count))
(churn-datum 2000)
(def d-delta (%sub (arena/count) d-before))
(println "region-read-meta-leak: syntax->datum object delta over 2000 = "
         d-delta)
(assert (%lt d-delta 100)
        (concat "(syntax->datum) leaks objects, delta=" (number->string d-delta)))

# ── (d) region-sweep witness ──────────────────────────────────────
# The region-level dual: each call's native-result region must be reclaimed, so the
# live region count returns to baseline. Catches a fix that allocates the result's
# region but forgets to free it (a region leak, vs the object leak (c) pins).
(churn-read 200)
(def rc-before (arena/region-count))
(churn-read 2000)
(def rc-delta (%sub (arena/region-count) rc-before))
(println "region-read-meta-leak: live region delta over 2000 reads = " rc-delta)
(assert (%lt rc-delta 50)
        (concat "(read) leaks regions, delta=" (number->string rc-delta)))

(println "region-read-meta-leak: ok")
