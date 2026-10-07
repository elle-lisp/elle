(elle/epoch 13)
# audited: 2026-09-30
# The direct-loop rows for scope reclamation, branch compensation, collections, strings and cells — one per-op thunk each.
#
# docs/impl/region/diagnostics.md
(def suite-direct
  [# scope reclamation
   ["discard-struct" (fn [j] {:x j :y (+ j 1)})]
   ["string-alloc" (fn [j] (string "iter-" j))]
   ["pair" (fn [j] (pair j (list)))]
   ["let-struct"
    (fn [j]
      (let [x {:iter j}]
        x))]
   ["traited"
    (fn [j]
      (let [t (with-traits @[1 2 3] {:tag :x})]
        (get (traits t) :tag)))]
   ["closure-template"
    (fn [j]
      (when (%not (%int? j)) (error :j))
      (let [f (fn [x] (%add x j))]
        (f 1)))]
   # Per-path branch compensation (src/hir/region/infer/compensate.rs). A value
   # live-in to a branch but used in only ONE arm is freed on the used path by its
   # in-arm decref AND on every other path by a compensating release at the dead
   # arm's head, so it reclaims on every path — not only the one reaching its last
   # use. Without it, a value whose sole use sits in a never-taken arm leaks 1/op.
   # Each probe forces the NO-USE arm to be the taken one (its `if` cond always
   # falls to the arm that does not reference the value), so a missing
   # compensation reads 1.
   ["branch-one-arm"
    (fn [j]
      (let [op (string "v" j)]
        (if (number? j) (%gt j 0) (string? op))))]
   ["branch-fresh-arm"
    (fn [j]
      (let [s {:k j}]
        (if (number? j) (%gt j 0) (get s :k))))]
   ["branch-nested"
    (fn [j]
      (let [op (string "v" j)]
        (if (number? j) (if (%lt j 999999) 1 (string? op)) 9)))]
   ["branch-error-arg" (fn [j] (check-arg (string "op" j) j))]
   # Comparison builtins reclaim: `check-comparable`'s op-name is an interned
   # keyword (no per-call alloc), and were it a heap string the compensation above
   # would still reclaim it — its uses sit only in the cold error arms.
   ["cmp-gt" (fn [j] (> j 0))] ["cmp-lt" (fn [j] (< j 0))]
   ["cmp-ge" (fn [j] (>= j 0))]
   ["fiber-drop"
    (fn [j]
      (let [f (fiber/new (fn [] 7) 2)]
        f))]
   ["fiber-resume"
    (fn [j]
      (let [f (fiber/new (fn [] 7) 2)]
        (fiber/resume f)))]
   # The fiber value installers (`fiber/resume`/`abort`/`cancel`/`emit`) declare
   # `Delivers`: the install into another fiber's signal slot counts its own
   # reference at runtime, so no arg clique. A `Mixed` declaration charges one
   # never-balancing `IncrefRegion` per heap-argument pair, which only a HEAP
   # payload arms — an immediate one leaves the pair unformed and measures nothing
   # either way. The control for the class (docs/impl/region/effects.md); the
   # four installers' inline faces are pinned by
   # tests/impl/region-fiber-install-clique-leak.lisp.
   ["fiber-deliver"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield 1)
                           2) |:yield|)]
        (fiber/resume f)
        (fiber/resume f [j])))] ["array-literal" (fn [j] [j (+ j 1) (+ j 2)])]
   ["mut-array" (fn [j] @[])]
   ["mut-array-push"
    (fn [j]
      (let [a @[]]
        (push a j)
        a))] ["mut-struct" (fn [j] @{:x j})]
   ## The stdlib `push` wrapper's `:@string` arm reclaims (rate 0). Two releases
   ## make it so: the `-mut` CONTAINER via `%string-push-mut` (a `MutableString` pass-through — per-arm
   ## container release + tail-retain suppression, like the `@array`/`@struct`/`@set` arms),
   ## and the byte-copy pushed-VALUE (`@string` copies the value's bytes rather than
   ## retaining its region, so `val` strands across the wrapper's arms; the compensation
   ## releases it per-arm from `funnel_bytecopy_value_sites`, sound because the byte-copy
   ## touched neither `val`'s incref nor its decref).
   ["mut-string"
    (fn [j]
      (let [s @""]
        (push s "x")
        s))]
   ["nested-loop"
    (fn [j]
      (def @k 0)
      (while (%lt k 10)
        {:x j :y k}
        (assign k (%add k 1))))]  # collection ops
    ["reduce" (fn [j] (reduce + 0 [1 2 3]))]
   ["fold" (fn [j] (fold (fn [a x] (+ a x)) 0 [1 2 3]))]
   # zip's column walks are cell-free top-level drivers threading `arrs`/`out` and
   # their accumulators as params (`zip-tuple-at`/`zip-build-array`/`zip-build-list`),
   # so the walks allocate no closure and no capture cell. Each driver is a walk
   # whose base case returns a heap argument while the recursive arm holds the
   # `decref_point`, so it rests on the per-path return frontier
   # (docs/impl/region/compensate.md); without it each call strands the argument it
   # hands back. A control at 0.
   ["zip" (fn [j] (zip [1 2] [3 4]))] ["sort" (fn [j] (sort [3 1 2]))]
   # `reverse` is a CLOSED control for the branch-arm release window
   # (docs/impl/region/window.md): its accumulator is named by every arm of the
   # trailing `(match t :array (freeze r) … _ r)`. Without the window the one
   # release lands in the last arm and every earlier one strands the whole
   # accumulator. A control at 0.
   ["reverse" (fn [j] (reverse [1 2 3]))]
   # `(rest array)` copies the tail into a fresh immutable array; its call-result
   # region reclaims on discard (rate 0). The trait-dispatched `Sequence:rest`
   # native allocates the slice into the outer `rest` call's OWN region (the
   # `dispatch_native_call` fresh-result invariant — a fresh native result lives
   # in the call's `alloc_region`, so the consumer's `DecrefValueRegion` frees
   # it). A separate boundary region would strand it. A control at 0 beside
   # `slice`/`to-array`: it rises if a boundary region strands the
   # slice (runtime::tests::ownership::
   # region_native_trait_dispatch_fresh_result_reclaims). `(rest list)` shares its
   # tail (also 0).
   ["rest-array-copy" (fn [j] (rest [1 2 3 4 5]))]
   # `distinct` is a control at 0 for the arm reading. Its dispatch is a `cond`
   # naming `coll` in every clause TEST, and a clause test is a conditional
   # position exactly as a clause body is (docs/impl/region/window.md). Read as
   # an unconditional position, the argument's one release sits in the LAST test
   # and every call taking an earlier body strands the whole input.
   ["distinct" (fn [j] (distinct [1 2 1 3]))]
   # `take`/`drop` are controls at 0 for the PER-PATH return frontier
   # (docs/impl/region/compensate.md;
   # tests/impl/region-return-arm-escape-leak.lisp). Both are `letrec` walks whose
   # base case returns a heap value while the recursive arm holds its
   # `decref_point`. Without the per-path frontier the returning arm carries a
   # return mint and no release, and each call strands what it hands back —
   # `drop` its whole input list even at n=0, `take` its reverse-scratch.
   ["take" (fn [j] (take 2 (list 1 2 3)))]
   ["drop" (fn [j] (drop 1 (list 1 2 3)))]
   # `group-by`/`frequencies`/`merge`/`each-list` are controls at 0 for the
   # release ROUTE reading: one binding owns a
   # region's route — the one whose init allocated it — so a second name bound from
   # the value refuses nothing (docs/impl/region/window.md). Each of these walks its
   # input with a reassigned cursor bound inside a type-dispatch arm; reading the
   # mutation off the cursor holds the whole input per call.
   ["group-by" (fn [j] (group-by odd? [1 2 3 4]))]
   ["frequencies" (fn [j] (frequencies [1 2 1 3]))]
   ["to-array" (fn [j] (->array (list 1 2 3)))]
   ["to-list" (fn [j] (->list [1 2 3]))] ["freeze" (fn [j] (freeze @[1 2 3]))]
   ["slice" (fn [j] (slice [1 2 3 4] 1 3))]
   # trailing nil keeps the body's value a discarded STATEMENT, as in a
   # while-loop body, where the alloc is never the loop's tail value
   ["keys-values"
    (fn [j]
      (keys {:a 1 :b 2})
      (values {:a 1 :b 2})
      nil)] ["merge" (fn [j] (merge {:a 1} {:b 2}))]
   ["struct-lit" (fn [j] {:x j :y (+ j 1)})]
   ["struct-get"
    (fn [j]
      (let [s {:x j}]
        s:x))]
   ["struct-put"
    (fn [j]
      (let [s @{:x 0}]
        (put s :x j)))]
   ["push-churn"
    (fn [j]
      (let [items @[]]
        (push items {:k j})))]
   # The capture-back-edge cycle: a container captured by a closure it holds
   # (`m ⊇ c` store, `c ⊇ m` capture). Per-region RC cannot collect the m↔c
   # cycle, and no region root can own it (the captured member's live decref
   # over-extends past the closure). The activation-owner cut reclaims the
   # INTRINSIC form of this shape (runtime::tests::ownership::
   # region_ownership_capture_back_edge_cycle_reclaims, without_stdlib /
   # %array-push). CLOSED for the full-stdlib form too: the `(push m c)` that
   # records the `m` contains `c` edge monomorphizes to `%push-array-mut`
   # cross-unit (mutable @array is a self-reclaiming op,
   # src/hir/typeinfer/monomorphize.rs), so the containment reaches the cut exactly
   # as the intrinsic form does — no wrapper to hide it.
   ["capture-backedge"
    (fn [j]
      (let [root @[]
            m @[]]
        (let [c (fn [] (length m))]
          (push m c)
          (c)
          (push root m)
          nil)))]
   # The transferred returned cycle: a helper builds an a<->b cycle and hands
   # its root back across the return frontier; the consumer discards it.
   # Per-region RC cannot collect the cycle (the interior back-edge outlives
   # every release) and no region root can own it (the root crosses the
   # frontier). The transfer cut (owner = the consuming activation's node)
   # reclaims it — rate 0
   # (runtime::tests::ownership::region_ownership_reclaims_returned_cycle_across_calls
   # pins it bounded).
   ["returned-cycle"
    (fn [j]
      (begin
        (cyc-mk)
        nil))]  # string ops + realistic patterns
    ["string-interp" (fn [j] (string "x=" j " y=" (+ j 1)))]
   # `concat` folds the extra arguments through `core-fold-step`, whose accumulator
   # is a returned parameter the recursive arm hands its callee only through the
   # combiner's RESULT. That point cannot reach the accumulator, so it owes no
   # funding edge and each displaced one is freed per step
   # (docs/impl/region/relocate.md). The 2-argument shape is `stdlib-concat` below; this is the
   # 3-argument one, where the fold actually recurses.
   ["concat" (fn [j] (concat "a" "b" "c"))]
   ["split" (fn [j] (string/split "a,b,c" ","))]
   ["join" (fn [j] (string/join ["a" "b" "c"] ","))]
   ["trim" (fn [j] (string/trim "  x  "))]
   ["replace" (fn [j] (string/replace "hello" "l" "r"))]
   ["num-to-str" (fn [j] (number->string j))] ["read" (fn [j] (read "42"))]
   ["call-chain" (fn [j] (helper-f (helper-g (helper-h j))))]
   # A fresh heap
   # value (`helper-g` result) stored into a cons via `(pair … …)` — a control
   # at 0 for the cons-store containment accounting. The alloc funnel
   # (`alloc_in_region` → `incref_cross_region_refs`) is the sole containment
   # incref for the `%pair`/`list` opcode (`handle_list`, src/vm/data.rs), exactly
   # as for `args_to_list` and every native list/array constructor. A second,
   # hand-written incref in `handle_list` double-counts each stored heap element
   # against the single free-time cascade decref — 1/op per heap member.
   # Soundness pinned by region-pair-heap-content-uaf.lisp.
   ["arg-result" (fn [j] (pair j (helper-g j)))]
   ["let-chain"
    (fn [j]
      (let [a (helper-h j)]
        (let [b (helper-g a)]
          b)))]
   # `each` over a statically-typed collection reclaims: the literal array's
   # `(match (type-of seq) …)` off-array arms are pruned (src/hir/typeinfer/prune.rs), so
   # seq lives only in the live arm. `each-manual` is the equivalent indexed loop.
   ["each-array"
    (fn [j]
      (each x in [1 2 3]
        x))]
   ["each-manual"
    (fn [j]
      (let [a [1 2 3]]
        (def @k 0)
        (while (%lt k 3)
          (get a k)
          (assign k (%add k 1)))))]
   # A CURSOR walked over a cons chain, whose cell's INIT carries a second
   # name: `xs` holds the chain head for the whole call. A cell donates its init
   # only where it is that value's sole holder, so the alias costs the donation —
   # and the cell counts the init instead, keeping the container model and the
   # STORE-SITE PIN it carries (docs/impl/region/bindings.md). Refusing the model
   # instead rides each step's release out to the cell's last use, so one release
   # covers the whole walk and every cons the cursor passed stays live.
   #
   # `each-manual` directly above is the same `while` loop over an ARRAY and
   # reads 0 either way — an index walk holds no cell — so the gap between the
   # two isolates the cell rather than the loop.
   ["list-cursor"
    (fn [j]
      (let [xs (list 1 2 3 4)
            @r xs
            @n 0]
        (while (not (empty? r))
          (assign n (%add n 1))
          (assign r (rest r)))
        n))]
   # The same walk with the alias taken AFTER the cell binding: `keep` is a
   # whole-value read of the container, so it takes a counted reference of its
   # own and the cell keeps its donation (docs/impl/region/reads.md).
   # `list-cursor` directly above is the same walk with the alias taken BEFORE,
   # where the counted-INIT route runs instead, so the gap between the two
   # isolates the route rather than the model.
   ["cell-alias-after"
    (fn [j]
      (let [@r (list 1 2 3 4)]
        (let [keep r]
          (def @n 0)
          (while (not (empty? r))
            (assign n (%add n 1))
            (assign r (rest r)))
          (list n (first keep)))))]
   # A branch inside a loop stores into ONE cell from both arms, so the cell has
   # two store sites and each arm allocates the value it stores. Each stored
   # value's producer release is discharged at the store that took THAT value
   # (docs/impl/region/bindings.md). Pinning both values at the cell's LAST store puts the first arm's
   # release inside the second arm, which that arm's path does not reach, so an
   # iteration repeating the first arm displaces the previous value from its own
   # ANF slot with nothing left to release it. `list-cursor` above is the
   # single-store control — one arm, one site, nothing to mis-pair.
   ["cell-arm-store"
    (fn [j]
      (let [@last (array 0 0)]
        (def @i 0)
        (while (%lt i 4)
          (if (%lt (%mod i 4) 2)
            (assign last (array i 7))
            (assign last (array i 9)))
          (assign i (%add i 1)))
        (get last 1)))] ["format" (fn [j] (string "iter " j " of " 100))]
   # The four-stage `split`/`map`/`filter`/`join` chain — a control at 0. Every stage
   # dispatches through a `cond` over its argument's type, so each call rests on
   # the clause-test reading `distinct` pins above.
   ["pipeline"
    (fn [j]
      (string/join (filter (fn [x] (not= x ""))
                           (map string/trim (string/split "a , b , c" ","))) ","))]
   ["each-list"
    (fn [j]
      (each x in (list 1 2 3)
        {:val x}))]
   # `map-while`/`filter-while`/`mapcat-while` are DISSOLUTION controls: a
   # non-capturing kernel
   # over a proven immutable array fuses to an inlined index-walk loop
   # (docs/impl/dissolution.md), so the stdlib op — and every per-call strand it
   # carries (the closure `map` mints for `f`, the `freeze` copy) — ceases to exist
   # and the rate is 0. The UN-fused op is gauged by `wrap-map` below, whose lambda
   # captures.
   ["map-while"
    (fn [j]
      (map (fn [x]
             (numeric!)
             (%add x 1)) [1 2 3]))]
   ["filter-while"
    (fn [j]
      (filter (fn [x]
                (numeric!)
                (%gt x 1)) [1 2 3]))]
   # The `mapcat` row reads the axis the fusion's own gauges do not: they weigh
   # `arena/total-allocs`, a cumulative count of allocation EVENTS, which a
   # retained object raises exactly as a reclaimed one does. Only a rate sees
   # retention, and the fan-out is the stage that has something to retain — the
   # array its function returns, one per INPUT element per call.
   ["mapcat-while" (fn [j] (mapcat (fn [x] [x x]) [1 2 3]))]
   # A control at 0 for the call-result naming rule. The walk inlines `f`'s
   # body, and the regions that walk yields — here the one the inner lambda's
   # closure+env live in — name the CALLEE's activation; the caller's temp holds
   # the call's own region instead (docs/impl/region/mechanism.md). Adopting them
   # makes the caller a second, nominal holder and drags the region's one release
   # onto a node the allocating path never reaches.
   ["nested-closure"
    (fn [j]
      (let [f (fn [] (fn [] j))]
        ((f))))] ["user-struct" (fn [j] (make-struct j))]
   ["user-string" (fn [j] (make-label j))] ["chain" (fn [j] (process j))]
   # The gauge for the UN-fused stdlib `map`: the kernel CAPTURES `k`, a shape loop
   # fusion declines (splicing a capture at the call site is out of scope), so the
   # real `map` runs and its per-call strands are measured. A control at 0: `map`
   # dispatches on its collection's type through a `cond`, whose later clause TESTS
   # are conditional positions for the argument's one release
   # (docs/impl/region/window.md). Its `push-accum` face is the same op driven into
   # a block-local accumulator.
   ["wrap-map"
    (fn [j]
      (let [k 1]
        (map (fn [x]
               (numeric!)
               (%add x k)) [1 2 3])))] ["factory" (fn [j] (t13proc j))]
   ["cond-factory" (fn [j] (t13cond j))] ["alias" (fn [j] (make-struct j))]
   ["nested-factory" (fn [j] (t13nested j))]
   ["struct-field"
    (fn [j]
      (the-mod:make j)
      (the-mod:label j))] ["heap-struct-field" (fn [j] (heap-mod:process j))]
   ["g-variant"
    (fn [j]
      (let [g (fn [] (%pair j j))]
        (g)))]
   ["bound-callee"
    (fn [j]
      (let [f t17-h]
        (f)))]
   ["break-skip"
    (fn [j]
      (block (let [a {:k j}]
               (let [b {:a j}]
                 (break)))))]
   ["store-wrapper" (fn [j] (t19-store t19s (string "v" j)))]
   ["fresh-env-cell" (fn [j] (t20-make-cell))]
   ["env-cell-read-arm" (fn [j] (t20-read-arm true))]
   ["shared-env-cell"
    (fn [j]
      (let [f (fn []
                (assign t20c (%add t20c 1))
                t20c)]
        (f)))]])

(println "── folded suite: direct-loop class ──")
(run-direct-loop suite-direct)
