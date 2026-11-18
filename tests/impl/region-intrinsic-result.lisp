(elle/epoch 15)
# audited: 2026-11-14
# The call-result region an allocating intrinsic gives its result is reclaimed on every tier.
# docs/impl/region/effects.md
#
# The allocating intrinsics %put / %del / %string-push give their result a
# call-result region (its OWN region, freed by a value-based DecrefValueRegion)
# like a native call, never the ambient/enclosing region. This pins that the
# per-call region is RECLAIMED:
# over a fixed window the `arena/region-count` delta stays bounded for both the
# immutable (fresh-copy) and mutable (in-place pass-through) cases, on every
# tier. A broken pass-through-retain or a missing DecrefValueRegion would strand
# one region per call -> an unbounded delta. Uses the raw %-intrinsics so the
# `Put` / `Del` / `IntrStringPush` instructions are exercised directly (the
# stdlib `put`/`del` wrappers dispatch on type first).
#
# The static pin is `put_intrinsic_gets_a_call_result_region` in
# src/hir/region/infer/tests/realalloc.rs: a walk that treats %put as
# region-transparent manufactures no region for it. This run-time check guards
# that the manufactured per-call region is actually freed.

(include-file "gauge/window.lisp")

# immutable args -> fresh copy each call; result consumed then dropped
(defn imm-put ()
  (%get (%put {:a 1} :b 2) :b))
(defn imm-del ()
  (%length (%del {:a 1 :b 2} :a)))
(defn imm-spush ()
  (%length (%string-push "ab" "x")))
# mutable args -> in-place mutation, pass-through result
(defn mut-put ()
  (%get (%put @{:a 1} :b 2) :b))
(defn mut-spush ()
  (%length (%string-push @"ab" "x")))

(def d-imm-put (measure-regions imm-put 200 4000))
(def d-imm-del (measure-regions imm-del 200 4000))
(def d-imm-spush (measure-regions imm-spush 200 4000))
(def d-mut-put (measure-regions mut-put 200 4000))
(def d-mut-spush (measure-regions mut-spush 200 4000))

(println "region-intrinsic-result deltas over 4000 iters:")
(println "  imm: put=" d-imm-put " del=" d-imm-del " string-push=" d-imm-spush)
(println "  mut: put=" d-mut-put " string-push=" d-mut-spush)

(assert (%lt d-imm-put 100)
        (concat "immutable %put strands a region per call, delta="
                (number->string d-imm-put)))
(assert (%lt d-imm-del 100)
        (concat "immutable %del strands a region per call, delta="
                (number->string d-imm-del)))
(assert (%lt d-imm-spush 100)
        (concat "immutable %string-push strands a region per call, delta="
                (number->string d-imm-spush)))
(assert (%lt d-mut-put 100)
        (concat "mutable %put strands a region per call, delta="
                (number->string d-mut-put)))
(assert (%lt d-mut-spush 100)
        (concat "mutable %string-push strands a region per call, delta="
                (number->string d-mut-spush)))
(println "region-intrinsic-result: ok")
