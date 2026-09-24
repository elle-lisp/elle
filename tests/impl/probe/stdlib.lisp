(elle/epoch 13)
# audited: 2026-09-30
# The stdlib per-call classes: transform scratch, the zip tower, the native-tail store family, and the read/copy primitives.
#
# docs/impl/region/diagnostics.md
# ── Stdlib / native-tail / discarded-tail leak classes ────────────────
# Three more leak classes read in the one dashboard (leak state read in one
# place), each row at 0.
#
# The native-tail and read rows read the region count, the second heap
# dimension — a class can leak whole REGIONS without growing the object count
# (a native fresh-result region whose contents are few). `r:stmt-run` drives a
# thunk b times as a discarded STATEMENT (non-tail), the while-loop shape a
# per-call leak needs to surface.
#
# The discarded-tail-return helpers take an unused `&opt` parameter. The graft
# refuses such a function, so each call stays a call whose tail returns a value
# (docs/impl/graft.md).
(defn ora-ret-first [xs &opt o]
  (first xs))
(defn ora-mk [x &opt o]
  {:v x})
(defn ora-ret-closure [x &opt o]
  (ora-mk x))

(println "── folded suite: stdlib / native-tail / discarded-tail canaries ──")

# The transform scratch. What a stdlib call can strand is INTERMEDIATE scratch,
# not the recursive helper (which reclaims — the `recur-local-*` probes read 0)
# and not, mostly, cons cells. `fold`/`reduce` `(->array coll)` once and
# INDEX-walk through the shared self-recursive `core-fold-step` driver
# (core.lisp), so neither a first/rest copy-scratch nor a per-call `go` closure
# exists.
#
# `stdlib-concat` and `stdlib-fold` are controls at 0. Two readings of one
# window close them. The accumulator
# `concat` fills is `push-all`'s returned parameter, whose release the branch-arm
# window anchors where every arm reaches it. And `core-fold-step`'s own accumulator
# is a returned parameter the recursive arm hands its callee only through the
# COMBINER's result — a point the callee cannot reach it at, which is exactly where
# no funding edge is owed (docs/impl/region/mechanism.md § "The callee's return
# mint, and why the point owes it nothing"), so each displaced accumulator is freed
# per step instead of stranded.
(r:drive "stdlib-concat" (r:stmt-run (fn [] (concat "a" "b"))))
(r:drive "stdlib-fold"
         (r:stmt-run (fn [] (fold (fn [_ b] b) nil (list "x" "y")))))

# ── HOF composition — the zip-tower witness ───────────────────────────
# `zip-tower` is a zip built as a TOWER of higher-order calls: it converts every
# input to a list (`map to-list`), then recurses building the result with `(map
# first lists)` AND `(map rest lists)` at every step, then rebuilds an array. It is
# `map`/`pair`/`reverse`/`push` stacked several deep, so it measures whether COMPOSING
# higher-order calls compounds the collection-builder over-keep — a rate that scales
# with composition DEPTH rather than staying a per-call constant. A control at 0.
#
# The trap is a placement one. Each of the tower's helpers is a cell-free
# self-recursive `letrec` closure whose demise the binder carries out to the
# `Letrec` node, and every stdlib entry point the tower calls dispatches through a
# branch whose arms tail-call out — so that release was emitted at a merge label no arm
# arrives at. The frame-exit relocation replicates it ahead of each arm's `TailCall`,
# which needs the closure's VALUE route, the slot its `letrec` binder recorded
# (docs/impl/region/mechanism.md § "Self-cancelling is a property of the ROUTE, not of
# the region's class"). One row holds on both tiers: the layers' arg-position
# closure-call results ride no retain that one tier holds and the other does not.
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
(r:drive "zip-tower" (r:stmt-run (fn [] (zip-tower [1 2] [3 4]))))

# The dispatch wrappers over an IMMUTABLE input. A call to `put` or `del` grafts,
# and the fold keeps the one `(type-of coll)` arm the proven type selects, so
# `(put {…} …)` is the direct `%put-struct` (docs/impl/graft.md). Controls at 0.
# The counter-factual: the whole wrapper ran, and it used `coll` in EVERY arm
# with one `decref_point` in one arm, so the other paths stranded the container
# reference and a redundant fresh-result retain.
(r:drive "native-tail-put-struct" (r:stmt-run (fn [] (put {:a 1} :b 2)))
         :on [r:regions])
(r:drive "native-tail-put-array" (r:stmt-run (fn [] (put [10 20] 0 99)))
         :on [r:regions])
(r:drive "native-tail-del-ctl" (r:stmt-run (fn [] (del {:a 1 :b 2} :a)))
         :on [r:regions])
# The store family beyond `put`/`del`: `push` and `add` on an immutable container
# take the same path. The counter-factual reads 1/op for an array or a set and
# 2/op for the byte-copy string push. These controls pin that the store family
# collapses as a whole, not just `put`.
(r:drive "native-tail-push-array" (r:stmt-run (fn [] (push [1 2] 3)))
         :on [r:regions])
(r:drive "native-tail-add-set" (r:stmt-run (fn [] (add (set 1 2) 3)))
         :on [r:regions])
(r:drive "native-tail-push-string" (r:stmt-run (fn [] (push "ab" "c")))
         :on [r:regions])
# The MUTABLE fresh-result funnels — `push` on a mutable @bytes (`%bytes-push`, no
# in-place variant, returns fresh) and `pop` on a mutable @string (`%pop-string`,
# returns a fresh grapheme). Their raw ops reclaim (0/op direct). The trap: they
# are not pass-through funnels, so the container compensation that closes
# `%push-array-mut`/`%put-struct-mut` does not cover them, and a whole wrapper
# around them strands the mutable container. The fold keeps the one arm here too.
(r:drive "native-tail-push-mut-bytes" (r:stmt-run (fn [] (push (@bytes 1 2) 3)))
         :on [r:regions])
(r:drive "native-tail-pop-mut-string" (r:stmt-run (fn [] (pop (@string "abc"))))
         :on [r:regions])

# ── The read/copy class ───────────────────────────────────────────────
# The container READ and single-value COPY primitives — `first`/`rest`/`get`/`has?`/
# `length`/`last`/`->array`/`->list`/`keys`/`values`/`slice`. Every one RECLAIMS
# (0/op): transform scratch is COMPOSITIONAL — it lives in the HOF/transform
# BODIES (`take`/`drop`/`reverse`/`concat`/`merge`/`distinct`/…, read in the
# direct-loop rows), never in a standalone read of a discarded result, even the
# tail-COPY `(rest arr)`. These are controls at 0: a read primitive that strands
# its result fails here.
(r:drive "read-first" (r:stmt-run (fn [] (first [1 2 3]))) :on [r:regions])
(r:drive "read-rest" (r:stmt-run (fn [] (rest [1 2 3]))) :on [r:regions])
(r:drive "read-last" (r:stmt-run (fn [] (last [1 2 3]))) :on [r:regions])
(r:drive "read-get-array" (r:stmt-run (fn [] (get [1 2 3] 0))) :on [r:regions])
(r:drive "read-get-struct" (r:stmt-run (fn [] (get {:a 1} :a))) :on [r:regions])
(r:drive "read-has-struct" (r:stmt-run (fn [] (has? {:a 1} :a))) :on [r:regions])
(r:drive "read-length" (r:stmt-run (fn [] (length [1 2 3]))) :on [r:regions])
(r:drive "read-toarray" (r:stmt-run (fn [] (->array (set 1 2)))) :on [r:regions])
(r:drive "read-tolist" (r:stmt-run (fn [] (->list [1 2 3]))) :on [r:regions])
(r:drive "read-keys" (r:stmt-run (fn [] (keys {:a 1 :b 2}))) :on [r:regions])
(r:drive "read-values" (r:stmt-run (fn [] (values {:a 1 :b 2}))) :on [r:regions])
(r:drive "read-slice" (r:stmt-run (fn [] (slice [1 2 3 4] 1 3))) :on [r:regions])

# Discarded tail-return: a function whose tail is a call (native pass-through
# `first`, or a closure), invoked for effect with the result DISCARDED. The
# fresh-value pass-through RECLAIMS (the move convention balances) — at 0 here;
# a STDLIB-allocated (`concat`) result is the `stdlib-concat` row above.
(r:drive "discard-passthrough"
         (r:stmt-run (fn [] (ora-ret-first (list {:k 1})))))
(r:drive "discard-closure" (r:stmt-run (fn [] (ora-ret-closure 1))))
