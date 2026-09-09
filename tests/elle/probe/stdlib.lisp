(elle/epoch 12)
# audited: 2026-09-08
# The stdlib per-call classes: transform scratch, the zip tower, the native-tail store family, and the read/copy primitives.
#
# docs/impl/region/diagnostics.md
# ── Stdlib / native-tail / discarded-tail leak classes ────────────────
# Three more leak classes pinned in the one dashboard (leak state
# read in one place). Each pin is the TRUE CURRENT rate, shrink-only: a fix LOWERS
# it.
#
# These canaries read `region-gauge`, the second heap dimension — a class can
# leak whole REGIONS without growing the object count (a native fresh-result
# region whose contents are few). `stmt-run` drives a thunk b times as a discarded
# STATEMENT (non-tail), the while-loop shape a per-call leak needs to surface.
# Native pass-through / closure tail-returns, for the discarded-tail-return class.
(defn ora-ret-first [xs]
  (first xs))
(defn ora-mk [x]
  {:v x})
(defn ora-ret-closure [x]
  (ora-mk x))

(println "── folded suite: stdlib / native-tail / discarded-tail canaries ──")

# Stdlib per-call leak (F1a — the transform-scratch retain). The leaked
# objects are INTERMEDIATE scratch, NOT the recursive helper (which reclaims — the
# `recur-local-*` probes read 0) and NOT, mostly, cons cells. `fold`/`reduce`
# `(->array coll)` once and INDEX-walk through the shared self-recursive
# `core-fold-step` driver (core.lisp), so neither the first/rest copy-scratch nor a
# per-call `go` closure exists to leak.
#
# `stdlib-concat` and `stdlib-fold` are CLOSED controls (undeclared, like
# `rest-array-copy`). Two readings of one window close them. The accumulator
# `concat` fills is `push-all`'s returned parameter, whose release the branch-arm
# window anchors where every arm reaches it. And `core-fold-step`'s own accumulator
# is a returned parameter the recursive arm hands its callee only through the
# COMBINER's result — a point the callee cannot reach it at, which is exactly where
# no funding edge is owed (docs/impl/region/mechanism.md § "The callee's return
# mint, and why the point owes it nothing"), so each displaced accumulator is freed
# per step instead of stranded. A regression to open must trip the completeness gate
# loudly.
(pin (measure-core "stdlib-concat" (stmt-run (fn [] (concat "a" "b")))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "stdlib-fold"
                   (stmt-run (fn [] (fold (fn [_ b] b) nil (list "x" "y"))))
                   count-gauge 100 6 60 0.4 0.5) 0)

# ── HOF composition — the zip-tower witness ───────────────────────────
# `zip-tower` is a zip built as a TOWER of higher-order calls: it converts every
# input to a list (`map to-list`), then recurses building the result with `(map
# first lists)` AND `(map rest lists)` at every step, then rebuilds an array. It is
# `map`/`pair`/`reverse`/`push` stacked several deep, so it measures whether COMPOSING
# higher-order calls compounds the collection-builder over-keep — a rate that scales
# with composition DEPTH rather than staying a per-call constant. It is a CLOSED
# control now (undeclared, like `rest-array-copy`), so a regression to open trips the
# completeness gate loudly rather than being absorbed under a root.
#
# The last mechanism it needed was a placement one. Each of the tower's helpers is a
# cell-free self-recursive `letrec` closure whose demise the binder carries out to the
# `Letrec` node, and every stdlib entry point the tower calls dispatches through a
# branch whose arms tail-call out — so that release was emitted at a merge label no arm
# arrives at. The frame-exit relocation replicates it ahead of each arm's `TailCall`,
# which needs the closure's VALUE route, the slot its `letrec` binder recorded
# (docs/impl/region/mechanism.md § "Self-cancelling is a property of the ROUTE, not of
# the region's class"). Shrink-only, and a SCALAR: the pin was a cross-tier [lo hi]
# range while the layers' arg-position closure-call results rode a ReturnValue retain
# the VM held and the JIT did not. Both tiers now measure the same rate, so the range
# collapses; re-open it as `[lo hi]` only if a tier span reappears.
(defn zip-tower [& colls]
  (letrec [to-list (fn (c)
                     (cond
                       (or (pair? c) (empty? c)) c
                       (array? c)
                         (letrec [loop (fn (i acc)
                                         (if (>= i (length c))
                                           (reverse acc)
                                           (loop (+ i 1) (pair (get c i) acc))))]
                           (loop 0 ()))
                       true (error {:error :type-error
                                    :reason :not-a-sequence
                                    :message "not a sequence"})))
           from-list (fn (lst orig)
                       (if (array? orig)
                         (let [arr @[]]
                           (each x in lst
                             (push arr x))
                           arr)
                         lst))
           zip-lists (fn (lists)
                       (if (any? empty? lists)
                         ()
                         (pair (map first lists) (zip-lists (map rest lists)))))]
    (if (empty? colls)
      ()
      (let* [lists (map to-list colls)
             result (zip-lists lists)]
        (from-list result (first colls))))))
(pin (measure-core "zip-tower" (stmt-run (fn [] (zip-tower [1 2] [3 4])))
                   count-gauge 100 6 60 0.4 0.5) 0)

# Dispatch-wrapper IMMUTABLE-input residual — CLOSED by cross-unit monomorphization
# (F1b; `hir/typeinfer/monomorphize.rs`). `put`/`del` on an immutable
# aggregate used to route through the whole wrapper — a `(match (type-of coll) …)` that
# used `coll` in EVERY arm with a single `decref_point` in one, stranding the owned-param
# container reference on the other paths PLUS a redundant fresh-result retain. The
# wrapper's definition lives in the stdlib unit, so the intra-unit monomorphize pass
# never reached a user call and only the container half was recoverable (by compensation).
# The cross-unit dispatch-wrapper registry now collapses `(put {…} …)` to the direct
# `%put-struct` at the proven immutable type — the wrapper, and every strand it carried,
# cease to exist, with no compensation gate. These are now CLOSED controls pinning that
# collapse (the one arm the registry leaves alone is a MUTABLE in-place `del`, which stays
# on its container compensation — `monomorphize.rs`, `is_mutable_container`).
(pin (measure-core "native-tail-put-struct" (stmt-run (fn [] (put {:a 1} :b 2)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-put-array" (stmt-run (fn [] (put [10 20] 0 99)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-del-ctl" (stmt-run (fn [] (del {:a 1 :b 2} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
# The store family beyond `put`/`del`: `push`/`add` on an immutable container had the
# SAME cross-unit wrapper strand, and leaked identically (measured 1/op for array/set,
# 2/op for the byte-copy string push, with the cross-unit path disabled) — but the F1b
# probe set never covered them, so the leak sat in the oracle's blind spot until the
# same registry collapse closed it. These are CLOSED controls pinning that the store
# family is handled generically, not just `put`.
(pin (measure-core "native-tail-push-array" (stmt-run (fn [] (push [1 2] 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-add-set" (stmt-run (fn [] (add (set 1 2) 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-push-string" (stmt-run (fn [] (push "ab" "c")))
                   region-gauge 100 6 60 0.4 0.5) 0)
# The MUTABLE fresh-result funnels — `push` on a mutable @bytes (`%bytes-push`, no
# in-place variant, returns fresh) and `pop` on a mutable @string (`%pop-string`,
# returns a fresh grapheme). Their raw ops reclaim (0/op direct), but the polymorphic
# wrapper stranded the mutable container: NOT a pass-through funnel, so the container
# compensation (which closes `%push-array-mut`/`%put-struct-mut`) never covered them.
# A matrix-coverage gap the whole-family sweep surfaced (push/pop on every type×
# mutability). Closed by extending cross-unit monomorphization to every self-reclaiming
# op on any mutability — only the mutable in-place `%del-*-mut` (open F5) is held back.
(pin (measure-core "native-tail-push-mut-bytes"
                   (stmt-run (fn [] (push (@bytes 1 2) 3))) region-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "native-tail-pop-mut-string"
                   (stmt-run (fn [] (pop (@string "abc")))) region-gauge 100 6
                   60 0.4 0.5) 0)

# ── The read/copy class ───────────────────────────────────────────────
# The container READ and single-value COPY primitives — `first`/`rest`/`get`/`has?`/
# `length`/`last`/`->array`/`->list`/`keys`/`values`/`slice`. Every one RECLAIMS
# (0/op): the F1a copy-scratch leak is COMPOSITIONAL — it lives in the HOF/transform
# BODIES (`take`/`drop`/`reverse`/`concat`/`merge`/`distinct`/…, pinned in the F1a
# suite above), never in a standalone read of a discarded result, even the tail-COPY
# `(rest arr)`. These are CLOSED controls: the family was previously unpinned, an
# oracle blind spot the whole-matrix sweep (the same audit that found the push/add and
# push-mut-bytes/pop-mut-string gaps) closed. A regression that makes any read
# primitive strand its result fails here loud.
(pin (measure-core "read-first" (stmt-run (fn [] (first [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-rest" (stmt-run (fn [] (rest [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-last" (stmt-run (fn [] (last [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-get-array" (stmt-run (fn [] (get [1 2 3] 0)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-get-struct" (stmt-run (fn [] (get {:a 1} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-has-struct" (stmt-run (fn [] (has? {:a 1} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-length" (stmt-run (fn [] (length [1 2 3])))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-toarray" (stmt-run (fn [] (->array (set 1 2))))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-tolist" (stmt-run (fn [] (->list [1 2 3])))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-keys" (stmt-run (fn [] (keys {:a 1 :b 2})))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-values" (stmt-run (fn [] (values {:a 1 :b 2})))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-slice" (stmt-run (fn [] (slice [1 2 3 4] 1 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)

# Discarded tail-return: a function whose tail is a call (native pass-through
# `first`, or a closure), invoked for effect with the result DISCARDED. The
# fresh-value pass-through RECLAIMS (the move convention balances) — pinned closed
# here; the residual is a jit-only reclamation gap for a STDLIB-allocated (`concat`)
# result, the `stdlib-concat` pin above.
(pin (measure-core "discard-passthrough"
                   (stmt-run (fn [] (ora-ret-first (list {:k 1})))) count-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "discard-closure" (stmt-run (fn [] (ora-ret-closure 1)))
                   count-gauge 100 6 60 0.4 0.5) 0)
