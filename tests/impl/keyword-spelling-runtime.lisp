(elle/epoch 13)
# audited: 2026-09-29
# The keywords the runtime coins for its tier, its configuration and a tier rejection all have a spelling.
# docs/impl/symbol.md
#
# A keyword IS its name hash, and a spelling the Rust runtime coins from a
# fixed string lives in the static vocabulary. The VM's tier, its
# configuration, and a tier rejection's context are all coined in Rust, and
# `vm/tier`, `vm/config` and `compile/run-on` are implementation extensions.
# tests/lang/keyword-spelling.lisp holds the language half.
#
# This file must not write any spelling it tests. A `:keyword` token
# teaches the reader's memo, and a literal would hand the runtime the very
# name it failed to record, so the assert would go green on a broken
# build. Every check below is over a value the runtime built, asking only
# whether a name came back at all.

(defn unspelled? [v]
  (string/contains? (string v) "#<keyword"))

(defn encodes? [v]
  (first (protect (json/serialize v))))

(defn failure [thunk]
  (second (protect (thunk))))

# ── The VM's own state ────────────────────────────────────────────────

(assert (not (unspelled? (vm/tier))) "the active tier has a spelling")
(assert (not (unspelled? (vm/config)))
        "every vm/config key and value has a spelling")
(assert (encodes? (vm/config)) "the vm config encodes as JSON")

# ── A tier rejection's context ────────────────────────────────────────

(def tier-failure
  (failure (fn [] (compile/run-on :nothing-of-this-name (fn [] 1)))))

(assert (not (unspelled? tier-failure)) "a tier rejection names its context")
(assert (encodes? tier-failure) "a tier rejection encodes as JSON")
