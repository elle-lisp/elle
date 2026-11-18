(elle/epoch 15)
# audited: 2026-11-17
# The stdlib per-call classes: transform scratch, the zip tower, the native-tail store family, and the read/copy primitives.
# docs/impl/region/diagnostics.md
# docs/impl/region/relocate.md
# docs/impl/region/replicate.md
#
# ── Stdlib / native-tail / discarded-tail leak classes ────────────────
# Three more leak classes, each row at 0. The native-tail and read rows read the
# region count: a class can leak whole REGIONS without growing the object count,
# as a native fresh-result region whose contents are few does. `r:stmt-run`
# drives a probe b times as a discarded STATEMENT, the shape a per-call leak
# needs to surface.
#
# The discarded-tail-return controls are a call's subject, so they run with the
# graft off in region-discarded-tail-return.lisp, not here.

(println "── folded suite: stdlib / native-tail / discarded-tail canaries ──")

# The transform scratch. What a stdlib call can strand is INTERMEDIATE scratch,
# not the recursive helper (which reclaims — the `recur-local-*` probes read 0)
# and not, mostly, cons cells. `fold`/`reduce` `(->array coll)` once and
# INDEX-walk through the shared self-recursive `core-fold-step` driver
# (core.lisp), so neither a first/rest copy-scratch nor a per-call `go` closure
# exists.
#
# `stdlib-concat` and `stdlib-fold` are controls at 0. Two readings of one
# window close them. The accumulator `concat` fills is `push-all`'s returned
# parameter, whose release the branch-arm window anchors where every arm
# reaches it. `core-fold-step`'s accumulator reaches its callee only through the
# COMBINER's result, where no funding edge is owed
# (docs/impl/region/relocate.md), so each displaced one is freed per step.
(r:drive "stdlib-concat" (r:stmt-run (fn [j] (concat "a" "b"))))
(r:drive "stdlib-fold"
         (r:stmt-run (fn [j] (fold (fn [_ b] b) nil (list "x" "y")))))

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
# `Letrec` node. Every stdlib entry point the tower calls dispatches through a
# branch whose arms tail-call out, so that release lands at a merge no arm
# reaches. The frame-exit relocation replicates it ahead of each arm's `TailCall`,
# which needs the closure's VALUE route, the slot its `letrec` binder recorded
# (docs/impl/region/replicate.md). One row holds on both tiers: the layers' arg-position
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
                       true
                         (error {:error :type-error
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
(r:drive "zip-tower" (r:stmt-run (fn [j] (zip-tower [1 2] [3 4]))))

# The dispatch wrappers over an IMMUTABLE input. A call to `put` or `del` grafts,
# and the fold keeps the one `(type-of coll)` arm the proven type selects, so
# `(put {…} …)` is the direct `%put-struct` (docs/impl/graft.md). Controls at 0.
# The counter-factual: the whole wrapper ran, and it used `coll` in EVERY arm
# with one `decref_point` in one arm, so the other paths stranded the container
# reference and a redundant fresh-result retain.
#
# Each container here names the op's index, so every op builds its own. A
# container of constants is built once per unit (docs/impl/hoist.md), and a
# strand of it moves no gauge. The allocation rows hold each to building it.
(def regions-allocs [r:regions r:allocs])
(r:drive "native-tail-put-struct" (r:stmt-run (fn [j] (put {:a j} :b 2)))
         :on regions-allocs)
(r:drive "native-tail-put-array" (r:stmt-run (fn [j] (put [j 20] 0 99)))
         :on regions-allocs)
(r:drive "native-tail-del-ctl" (r:stmt-run (fn [j] (del {:a j :b 2} :a)))
         :on regions-allocs)
# The store family beyond `put`/`del`: `push` and `add` on an immutable container
# take the same path. The counter-factual reads 1/op for an array or a set and
# 2/op for the byte-copy string push. These controls pin that the store family
# collapses as a whole, not just `put`.
(r:drive "native-tail-push-array" (r:stmt-run (fn [j] (push [j 2] 3)))
         :on regions-allocs)
(r:drive "native-tail-add-set" (r:stmt-run (fn [j] (add (set j -1) 3)))
         :on regions-allocs)
(r:drive "native-tail-push-string"
         (r:stmt-run (fn [j] (push (string "ab" j) (string "c" j))))
         :on regions-allocs)
# The MUTABLE fresh-result funnels — `push` on a mutable @bytes (`%bytes-push`, no
# in-place variant, returns fresh) and `pop` on a mutable @string (`%pop-string`,
# returns a fresh grapheme). Their raw ops reclaim (0/op direct). The trap: they
# are not pass-through funnels, so the container compensation that closes
# `%push-array-mut`/`%put-struct-mut` does not cover them, and a whole wrapper
# around them strands the mutable container. The fold keeps the one arm here too.
(r:drive "native-tail-push-mut-bytes"
         (r:stmt-run (fn [j] (push (@bytes 1 2) 3))) :on [r:regions])
(r:drive "native-tail-pop-mut-string"
         (r:stmt-run (fn [j] (pop (@string "abc")))) :on [r:regions])

# ── The read/copy class ───────────────────────────────────────────────
# The container READ and single-value COPY primitives — `first`/`rest`/`get`/`has?`/
# `length`/`last`/`->array`/`->list`/`keys`/`values`/`slice`. Every one RECLAIMS
# (0/op): transform scratch is COMPOSITIONAL — it lives in the HOF/transform
# BODIES (`take`/`drop`/`reverse`/`concat`/`merge`/`distinct`/…, read in the
# direct-loop rows), never in a standalone read of a discarded result, even the
# tail-COPY `(rest arr)`. These are controls at 0: a read primitive that strands
# its result fails here. A read whose result is an immediate builds nothing but
# its container, so that container names the op's index, as above.
(r:drive "read-first" (r:stmt-run (fn [j] (first [j 2 3]))) :on regions-allocs)
(r:drive "read-rest" (r:stmt-run (fn [j] (rest [1 2 3]))) :on [r:regions])
(r:drive "read-last" (r:stmt-run (fn [j] (last [j 2 3]))) :on regions-allocs)
(r:drive "read-get-array" (r:stmt-run (fn [j] (get [j 2 3] 0)))
         :on regions-allocs)
(r:drive "read-get-struct" (r:stmt-run (fn [j] (get {:a j} :a)))
         :on regions-allocs)
(r:drive "read-has-struct" (r:stmt-run (fn [j] (has? {:a j} :a)))
         :on regions-allocs)
(r:drive "read-length" (r:stmt-run (fn [j] (length [j 2 3]))) :on regions-allocs)
(r:drive "read-toarray" (r:stmt-run (fn [j] (->array (set 1 2))))
         :on [r:regions])
(r:drive "read-tolist" (r:stmt-run (fn [j] (->list [1 2 3]))) :on [r:regions])
(r:drive "read-keys" (r:stmt-run (fn [j] (keys {:a 1 :b 2}))) :on [r:regions])
(r:drive "read-values" (r:stmt-run (fn [j] (values {:a 1 :b 2})))
         :on [r:regions])
(r:drive "read-slice" (r:stmt-run (fn [j] (slice [1 2 3 4] 1 3)))
         :on [r:regions])
