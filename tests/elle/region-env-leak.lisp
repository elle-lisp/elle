(elle/epoch 12)
# Counterfactual: the per-call closure-env region leak.
#
# A non-tail closure call mints a fresh "env region" in `call_inner`
# (`new_runtime_region_for_call_slot`, src/vm/core.rs) and hands it to
# `populate_env` (src/vm/env.rs). That region holds the call's env-cell
# allocations:
#   - lboxes for mutated captured params (`@x`),
#   - cells for captured mutable locals,
#   - the rest-arg cons list for a variadic `(& rest)`.
#
# The Call instruction's single static region slot is OVERLOADED: the region
# analysis (src/hir/regions.rs) only ever registers it as the call *result*
# region (`call_result_regions`), so the lowerer emits a value-based
# `DecrefValueRegion` on the RETURNED value at the result's decref_point. The
# env region is a different runtime region entirely — minted fresh, never
# recorded in any `activation_region_map`, never targeted by a `DecrefRegion`.
# Its initial RC=1 therefore has no release path. It is only partially
# reclaimed via cascade when a nested closure that captured one of its cells
# is freed (dropping the closure->cell edge); the initial reference always
# leaks. When nothing captures its cells (the variadic rest-arg case) it
# leaks whole.
#
# Net: every closure call whose `populate_env` allocates into the env region
# leaks at least one region (and its objects) per call. docs/impl/region-rules.md Rule 8
# ("Nothing leaks but true process-lifetime roots"). Confirmed a pure leak,
# not a UAF, under `--trace=guardfree` (no fault — the regions accumulate, the
# pages are never freed, so nothing reads freed memory).
#
# RED before the env-region ownership fix, GREEN after. The `plain` controls
# (a non-capturing, non-variadic call) are bounded NOW — they prove the leak
# is the env-region allocation itself, not the measurement harness.

# ── subjects ──────────────────────────────────────────────────────

# (a) Variadic: the rest list `(1 2 3)` is consed into the env region by
# `args_to_list` and never released (one region + three conses per call).
(defn variadic (& xs)
  (length xs))

# (b) Mutated captured param: `@x` needs_capture (captured AND mutated), so
# `populate_env` wraps it in an lbox allocated into the env region. The
# returned inner closure captures that lbox; when the closure dies, cascade
# drops the closure->lbox edge but the env region's initial RC=1 remains.
(defn counter (@x)
  (fn ()
    (assign x (%add x 1))
    x))

# (c) Captured mutable LOCAL, minted fresh per `make-acc` call: `@total` is a
# captured-and-mutated local, so `populate_env`'s local-cell path allocates
# its cell into make-acc's env region. Unlike a captured local whose lbox is
# created ONCE (outside a loop) and reused, here a NEW env region is minted
# each call.
(defn make-acc ()
  (def @total 0)
  (fn (x)
    (assign total (%add total x))
    total))

# (d) The same captured-mutated-param closure wrapped in a fiber, built fresh
# per iteration. The env-region leak is amplified by the fiber/closure the
# fiber/new path allocates; this isolates the per-call closure-env
# contribution from a bare fiber-while (which is bounded). NON-tail
# position: `fiber/resume` is inside the while body, not a function tail (a
# tail call would route through `tail_call_inner`, a different region path).
(defn req-fiber (i)
  (let [f (fiber/new (counter i) 1)]
    (fiber/resume f)))

# (e) `&keys` collects keyword args into a struct (VarargKind::Struct,
# `args_to_struct_static` in src/vm/env.rs). The collected struct leaks one
# OBJECT per call — and, insidiously, `arena/region-count` does NOT catch it
# (the struct does not land in a fresh per-call region), so only an
# object-count witness sees it.
(defn reqkeys (&keys opts)
  (length opts))

# (f) `&named @flag`: a mutable named parameter. Its lbox leaks one OBJECT per
# call; like (e), region-count is flat — object-count is the only witness.
(defn reqnamed (&named @flag)
  flag)

# (g) `&opt` captured+mutated, called WITHOUT the optional arg: `populate_env`
# fills the missing optional slot (push_param with Value::NIL) and the
# captured-mutated `@b` still gets an lbox in the env region — region + object
# leak. Exercises the optional/nil-fill arm of `populate_env`.
(defn optfn (a &opt @b)
  (fn ()
    (assign b (%add a 1))
    b))

# (h) Transitive multi-level capture: `@x` is captured through two closure
# levels. `capt-outer`'s env-region lbox leaks per call regardless of the
# capture depth.
(defn capt-outer (@x)
  (fn ()
    (fn ()
      (assign x (%add x 1))
      x)))

# Control: no env-region allocation (params copied by value, no rest arg).
(defn plain (a b)
  (%add a b))

# ── measurement ───────────────────────────────────────────────────
# `arena/region-count` = live physical regions; `arena/count` = live objects.
# Each delta across a loop body is exactly what the loop leaked: ~0 when
# reclamation is correct, ~n (regions) / ~k*n (objects) for a per-iteration
# leak. Deltas are measured around the loop only; the surrounding defs/returns
# allocate outside the measured window.

(defn variadic-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (variadic 1 2 3)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn variadic-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (variadic 1 2 3)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn counter-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (def c (counter i))
    (c)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn counter-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (def c (counter i))
    (c)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn acc-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (def a (make-acc))
    (a 1)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn acc-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (def a (make-acc))
    (a 1)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn fiber-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (req-fiber i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn fiber-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (req-fiber i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn keys-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (reqkeys :a 1 :b 2)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn named-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (reqnamed :flag 7)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn opt-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (def c (optfn i))
    (c)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn opt-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (def c (optfn i))
    (c)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn trans-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (def g ((capt-outer i)))
    (g)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn trans-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (def g ((capt-outer i)))
    (g)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn plain-region-leak [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (plain 1 2)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn plain-object-leak [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (plain 1 2)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# Compute every delta first (so the diagnostic prints all magnitudes even
# though the first failing assert aborts the run).
(def p-reg (plain-region-leak 2000))
(def p-obj (plain-object-leak 2000))
(def v-reg (variadic-region-leak 2000))
(def v-obj (variadic-object-leak 2000))
(def c-reg (counter-region-leak 2000))
(def c-obj (counter-object-leak 2000))
(def a-reg (acc-region-leak 2000))
(def a-obj (acc-object-leak 2000))
(def f-reg (fiber-region-leak 2000))
(def f-obj (fiber-object-leak 2000))
(def k-obj (keys-object-leak 2000))
(def n-obj (named-object-leak 2000))
(def o-reg (opt-region-leak 2000))
(def o-obj (opt-object-leak 2000))
(def t-reg (trans-region-leak 2000))
(def t-obj (trans-object-leak 2000))

(println "region-env-leak over 2000 iters (region/object deltas):")
(println "  plain    reg=" p-reg " obj=" p-obj)
(println "  variadic reg=" v-reg " obj=" v-obj)
(println "  counter  reg=" c-reg " obj=" c-obj)
(println "  acc      reg=" a-reg " obj=" a-obj)
(println "  fiber    reg=" f-reg " obj=" f-obj)
(println "  &keys              obj=" k-obj)
(println "  &named             obj=" n-obj)
(println "  &opt     reg=" o-reg " obj=" o-obj)
(println "  trans    reg=" t-reg " obj=" t-obj)

# Controls: a plain call leaks neither regions nor objects. Bounded NOW; these
# guard the measurement harness against false positives.
(assert (%lt p-reg 50)
        (concat "control: plain call leaks regions, delta="
                (number->string p-reg)))
(assert (%lt p-obj 50)
        (concat "control: plain call leaks objects, delta="
                (number->string p-obj)))

# Witness (a): variadic rest-arg list — one env region + its conses per call.
(assert (%lt v-reg 50)
        (concat "variadic (& rest) leaks one env region per call, delta="
                (number->string v-reg)))
(assert (%lt v-obj 50)
        (concat "variadic (& rest) leaks rest-arg conses per call, delta="
                (number->string v-obj)))

# Witness (b): mutated captured param — one env region + its lbox per call.
(assert (%lt c-reg 50)
        (concat "closure capturing a mutated param (@x) leaks one env region "
                "per call, delta=" (number->string c-reg)))
(assert (%lt c-obj 50)
        (concat "closure capturing a mutated param (@x) leaks its lbox per "
                "call, delta=" (number->string c-obj)))

# Witness (c): captured mutable local minted fresh per call.
(assert (%lt a-reg 50)
        (concat "closure capturing a fresh mutable local leaks one env region "
                "per call, delta=" (number->string a-reg)))
(assert (%lt a-obj 50)
        (concat "closure capturing a fresh mutable local leaks its cell per "
                "call, delta=" (number->string a-obj)))

# Witness (d): the same capturing closure through a per-call fiber.
(assert (%lt f-reg 50)
        (concat "fiber-wrapped capturing closure leaks regions per call, delta="
                (number->string f-reg)))
(assert (%lt f-obj 50)
        (concat "fiber-wrapped capturing closure leaks objects per call, delta="
                (number->string f-obj)))

# Witness (e): `&keys` struct rest-arg — an OBJECT leak region-count misses.
(assert (%lt k-obj 50)
        (concat "&keys struct rest-arg leaks one object per call (region-count "
                "does not catch it), delta=" (number->string k-obj)))

# Witness (f): `&named @flag` mutable named param lbox — OBJECT leak.
(assert (%lt n-obj 50)
        (concat "&named mutable param leaks its lbox per call (region-count "
                "does not catch it), delta=" (number->string n-obj)))

# Witness (g): `&opt` captured-mutated param, nil-filled slot.
(assert (%lt o-reg 50)
        (concat "&opt captured param leaks one env region per call, delta="
                (number->string o-reg)))
(assert (%lt o-obj 50)
        (concat "&opt captured param leaks its lbox per call, delta="
                (number->string o-obj)))

# Witness (h): transitive multi-level capture of a mutated param.
(assert (%lt t-reg 50)
        (concat "transitive capture leaks one env region per call, delta="
                (number->string t-reg)))
(assert (%lt t-obj 50)
        (concat "transitive capture leaks its lbox per call, delta="
                (number->string t-obj)))

(println "region-env-leak: ok")
