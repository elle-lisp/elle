(elle/epoch 12)
# Runtime `(eval 'DATA)` of QUOTED COMPOUND DATA — a list, array, or nested
# structure of immediates — compiles that form with the quoted structure as an
# ordinary allocation: a recursive `MaterializeConst` template (plain compile-time
# data) materializes a FRESH structure into the literal's OWN solver-assigned
# region on every execution, freed at its `decref_point` by normal RC. So the
# whole structure shares one reclaimable region (an immutable aggregate), the
# eval'd result is reclaimable, and repeated eval stays bounded.
# docs/impl/region/rules.md Rule 8; docs/impl/region/model.md § "Constants lower
# as ordinary allocations".
#
# This is the quoted-DATA sibling of region-eval-leak.lisp (which pins the
# STRING-literal and closure-TEMPLATE witnesses).

# ── (a) attribution witness ───────────────────────────────────────
# A quoted compound literal an eval returns is an ordinary allocation in a real
# reclaimable region (id >= 2), not an immediate (id 0).
(def reg (arena/region-of (eval (quote (quote (a b c))))))
(assert (%ge reg 2)
        (concat "eval'd quoted list must be an ordinary allocation in a real "
                "reclaimable region — got " (number->string reg)))

# nested quoted data (list containing a string and an array) — every heap node of
# the structure shares the literal's one region, all reclaimable.
(def regn (arena/region-of (eval (quote (quote (1 "two" [3 4]))))))
(assert (%ge regn 2)
        (concat "eval'd nested quoted data must be a real reclaimable region — got "
                (number->string regn)))

# ── (b) value correctness ─────────────────────────────────────────
# Materializing fresh from a template must reproduce the quoted datum exactly.
(assert (= (eval (quote (quote (a b c)))) (quote (a b c)))
        "eval of a quoted list materializes the wrong value")
(assert (= (eval (quote (quote (1 "two" [3 4])))) (quote (1 "two" [3 4])))
        "eval of nested quoted data materializes the wrong value")

# ── (c) object-count leak witness ─────────────────────────────────
# Warm one-time effects, then measure per-iteration growth over a fixed window.
# A bounded eval leaves the count flat; a per-eval leak would grow ~3 objects/iter
# (three cons cells per `(a b c)`).
(defn eval-q (n)
  (var i 0)
  (while (%lt i n)
    (eval (quote (quote (a b c))))
    (assign i (%add i 1))))

(eval-q 200)
(def c-before (arena/count))
(eval-q 2000)
(def c-delta (%sub (arena/count) c-before))
(println "region-eval-quoted-data-leak: object delta over 2000 evals = " c-delta)
(assert (%lt c-delta 50)
        (concat "eval of quoted data leaks objects, delta="
                (number->string c-delta)))

# ── (d) region-sweep witness ──────────────────────────────────────
# The region-level dual: each eval's per-literal region must be reclaimed, so the
# live region count returns to baseline. Catches a fix that allocates the
# literal's region but forgets to free it (a region leak).
(eval-q 200)
(def rc-before (arena/region-count))
(eval-q 2000)
(def rc-delta (%sub (arena/region-count) rc-before))
(println "region-eval-quoted-data-leak: live region delta over 2000 evals = "
         rc-delta)
(assert (%lt rc-delta 50)
        (concat "eval of quoted data leaks regions, delta="
                (number->string rc-delta)))

(println "region-eval-quoted-data-leak: ok")
