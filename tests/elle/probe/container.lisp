(elle/epoch 12)
# audited: 2026-09-08
# Persistent fn-local containers, the loop-carried accumulator a function returns, and the captured accumulator a builder fills.
#
# docs/impl/region/diagnostics.md
# ── Persistent fn-local containers ────────────────────────────────────
# The container is `def`'d fn-local INSIDE the run-block (the faithful shape — a
# captured let-local or module binding hits a different region path) and reused
# across the block's ops. `push-outer` reclaims (rate 0): a block-local accumulator
# is freed at the block's return once the `push` wrapper stops stranding its
# owned-param reference (F1b container compensation) — its earlier per-op growth was
# that over-keep, not genuine retention (the gauge-live discriminator uses a
# MODULE-level sink, `probe-disc`, and is unaffected). `push-accum` is the same
# accumulator fed the per-op `map` scratch (§ F1a), and reclaims for the same
# reason once that scratch does: `map` dispatches through a `cond` whose later
# clause TESTS held the collection's one release, and a clause test is a
# conditional position exactly as a clause body is
# (docs/impl/region/mechanism.md § "An arm is a conditional position, not a
# syntactic arm body"). Its kernel CAPTURES `k` deliberately — a capture declines
# loop fusion, so the real stdlib `map` runs and there is a per-op scratch to
# measure; a fusable kernel has none, which is what the dissolution controls
# (`map-while`) measure. A CLOSED control now, undeclared like `rest-array-copy`.
# `struct-outer` is the fn-local reassign-1-slot control: a loop-carried cell whose
# content is re-minted every iteration, bounded by the overwrite + demise pair (F5).
# `string-outer`/`append-outer` are CLOSED controls for the same close
# `stdlib-concat` gauges: each iteration's `concat`/`append` returns `push-all`'s
# accumulator parameter, and the branch-arm window anchors its release where every
# arm reaches it. Their rate was always flat per-iter, never accumulator growth, so
# a regression to open is a per-call strand and must trip the completeness gate.
(println "── folded suite: persistent containers ──")
(pin (measure-core "put-overwrite"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:key 0})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :key (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "set-array"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @a @[(string "i")])
                     (def @j 0)
                     (while (%lt j b)
                       (put a 0 (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-struct"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:data nil})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :data {:iter j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "roster"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @tr @{:pnl 0 :trades 0 :label ""})
                     (def @j 0)
                     (while (%lt j b)
                       (put tr :pnl (%add j 100))
                       (put tr :trades (%add j 1))
                       (put tr :label (string "t-" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:x 0})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :x (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "push-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc @[])
                     (def @j 0)
                     (while (%lt j b)
                       (push acc {:x j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "push-accum"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc @[])
                     (def @j 0)
                     (def k 1)
                     (while (%lt j b)
                       (push acc
                             (map (fn [x]
                                    (numeric!)
                                    (%add x k)) [1 2 3]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "struct-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @last nil)
                     (def @j 0)
                     (while (%lt j b)
                       (assign last {:x j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "string-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s "")
                     (def @j 0)
                     (while (%lt j b)
                       (assign s (concat s "x"))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "append-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc [])
                     (def @j 0)
                     (while (%lt j b)
                       (assign acc (append acc [j]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)

# ── The loop-carried accumulator a function RETURNS ───────────────────
# `loop-acc-return` builds a list by reassigning a local across a `while` and
# hands it back; `recur-acc-return` computes the same list by threading the
# accumulator as a parameter to a self-recursive binding. Same value, same
# allocations, one difference: which construct carries the accumulator, so the
# pair prices the rewrite a programmer would otherwise have to make.
#
# The returned binding takes the 1-slot-container model, so each value the loop
# displaces dies at the overwrite and the final content leaves with the caller —
# the `Return`'s mint pays for the caller's reference and the cell's content drop,
# emitted after that mint, releases the cell's. Withhold the container half and
# each stored value is protected only by its producer's one release, which the
# returned-region extension drags out to the `Return`: it names whatever the
# producer's ANF slot holds LAST and every earlier value is stranded, one region
# per trip. Neither probe cares whether the accumulated values are
# self-referential (a cons chain and a fresh string per iteration read alike), and
# neither is about closure capture — a helper closing over the accumulator, or
# over a mutable input container, is the `capture-acc-*` pair below.
(defn loop-acc-return-shape [n]
  (def @acc ())
  (def @i 0)
  (while (%lt i n)
    (assign acc (pair i acc))
    (assign i (%add i 1)))
  acc)
(def recur-acc-return-step
  (fn [i n acc]
    (if (%lt i n) (recur-acc-return-step (%add i 1) n (pair i acc)) acc)))
(defn recur-acc-return-shape [n]
  (recur-acc-return-step 0 n ()))
(pin (measure "loop-acc-return" (fn [j] (length (loop-acc-return-shape 4))) 100
              6 60 0.4 0.5) 0)
(pin (measure "recur-acc-return" (fn [j] (length (recur-acc-return-shape 4)))
              100 6 60 0.4 0.5) 0)

# ── The captured mutable accumulator — the shape a builder is WRITTEN in ──
# Every container probe above drives its accumulator from a bare `while` in the
# same scope. The everyday builder does not: it names the walk, and that helper
# CAPTURES the mutable accumulator it fills. Both realizations are here, because
# they take different region paths — `capture-acc-letrec` closes a local
# `letrec` helper over `out` (a capture cell plus a closure per call),
# `capture-acc-while` closes a plain `let`-bound `fn` over it — and both must be
# bounded on the SAME terms as the uncaptured form, or a builder is only
# leak-free when its author threads the accumulator through parameters instead.
# `thread-acc-param` is that parameter-threaded alternative, kept beside them as
# the discriminator: were the captured forms to regress, this one would stay at 0
# and the difference would be exactly the cost of the rewrite. Closed controls,
# undeclared like `rest-array-copy`, so a regression trips the completeness gate
# rather than being absorbed under a root. The payload is a heap string per push,
# so a stranded accumulator shows as element growth and not merely one region.
(def thread-acc-driver
  (fn [out i n]
    (when (%lt i n)
      (push out (string "e" i))
      (thread-acc-driver out (%add i 1) n))))
(pin (measure-core "capture-acc-letrec"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]]
                         (letrec [go (fn [i]
                                       (when (%lt i 4)
                                         (push out (string "e" i))
                                         (go (%add i 1))))]
                           (go 0))
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "capture-acc-while"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]
                             fill (fn [n]
                                    (def @i 0)
                                    (while (%lt i n)
                                      (push out (string "e" i))
                                      (assign i (%add i 1))))]
                         (fill 4)
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "thread-acc-param"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]]
                         (thread-acc-driver out 0 4)
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
