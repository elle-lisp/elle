(elle/epoch 15)
# audited: 2026-11-18
# A tail call builds its callee's environment values in regions of their own, so a loop of tail calls leaves no object behind.
# docs/impl/region/rules.md
#
# A call in tail position builds the callee's environment through
# `pending_tail_call_to` (src/vm/env.rs), apart from the non-tail builder.
# Each value `populate_env` puts there, a cell for a mutated captured
# parameter or the rest list of a variadic, takes a fresh region from
# `env_value_region`, and the callee releases it at its last use.
#
# The counter-factual: a tail path that put every activation's values into one
# region reused across calls. That breaks Rule 6 (no commingling) and Rule 8
# (no leaks), and its signature is distinct: `arena/region-count` stays flat,
# one region reused, while `arena/count` grows with each call. So the object
# count is the witness here and the region count the foil.

# Tail-position subjects: each function's body IS a single call (tail call).
(defn variadic (& xs)
  (length xs))

(defn counter (@x)
  (fn ()
    (assign x (+ x 1))
    x))

(defn plain (a b)
  (%add a b))

# wrap-* tail-call the subject; `pending_tail_call_to` builds the callee's env.
(defn wrap-variadic (k)
  (variadic 1 2 3))

(defn wrap-counter (k)
  (counter k))

# Control: a tail call whose callee has NO env allocations (no rest arg, no
# captured-mutated params) commingles nothing.
(defn wrap-plain (k)
  (plain 1 2))

# ── measurement ───────────────────────────────────────────────────
# Two scales (50 and 2000); object count is the witness, region count the
# foil (it would stay flat through a commingling).

(defn wv-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-variadic 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn wv-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (wrap-variadic 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn wc-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-counter i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn wp-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-plain 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(def p-obj (wp-obj 2000))
(def v-obj-50 (wv-obj 50))
(def v-obj-2k (wv-obj 2000))
(def v-reg-2k (wv-reg 2000))
(def c-obj-50 (wc-obj 50))
(def c-obj-2k (wc-obj 2000))

(println "region-tail-env-commingle:")
(println "  wrap-plain    (control) obj 2000=" p-obj)
(println "  wrap-variadic obj 50=" v-obj-50 " 2000=" v-obj-2k
         " (region delta 2000=" v-reg-2k ")")
(println "  wrap-counter  obj 50=" c-obj-50 " 2000=" c-obj-2k)

# Control: a tail call with no env allocations leaks nothing.
(assert (%lt p-obj 50)
        (concat "control: plain tail call leaks objects, delta="
                (number->string p-obj)))

# Witness: the tail-called variadic's rest-arg conses are freed per call. In one
# reused region they would accumulate, with the region count flat.
(assert (%lt v-obj-2k 100)
        (concat "tail-call variadic env commingles/leaks objects (region-count "
                "flat at " (number->string v-reg-2k) "): obj 50="
                (number->string v-obj-50) " 2000=" (number->string v-obj-2k)))

# Witness: the tail-called counter's captured-param lbox is freed per call.
(assert (%lt c-obj-2k 100)
        (concat "tail-call counter env commingles/leaks objects: obj 50="
                (number->string c-obj-50) " 2000=" (number->string c-obj-2k)))

(println "region-tail-env-commingle: ok")
