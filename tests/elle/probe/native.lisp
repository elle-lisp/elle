(elle/epoch 12)
# audited: 2026-09-08
# Native results produced by running compiled code, the byte gauge, and the value-survival pins no rate can make.
#
# docs/impl/region/diagnostics.md
# ── Thunk-run native results ──────────────────────────────────────────
# A native can produce its result by running compiled code on the driving VM:
# `import` runs the module body, and `arena/allocs` runs the measured thunk.
# Such a result already carries its return mint. The dispatch pass-through
# retain must not fund the caller a second time (`result_minted`,
# docs/impl/region/effects.md § "Native region effects"). `arena/allocs`
# embeds its thunk's result in a fresh pair, so the boundary consumes the
# mint after the pair's alloc-scan counts the embedding. Both probes are
# CLOSED controls (undeclared, like `rest-array-copy`). Before the
# accounting fix each read ~3/op — the returned closure, its letrec arena,
# and a capture cell, stranded per call — so a regression to open trips the
# completeness gate loudly. The discarded-statement shape needs the DIRECT
# while run-block (a thunk's return convention would mask the over-keep).
(def import-module-dir (file/mktempdir))
(def import-module-path (string import-module-dir "/oracle-import-mod.lisp"))
(spit import-module-path
      (string "(elle/epoch 12)\n" "(defn f1 [x] (+ x 1))\n"
              "(fn [] {\"f1\" f1})\n"))
(defn import-thunk []
  (defn g1 [x]
    (+ x 1))
  (fn [] {"g1" g1}))
(println "── folded suite: thunk-run native results ──")
(pin (measure-core "import-result"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (begin
                         (import import-module-path)
                         nil)
                       (assign j (%add j 1)))) count-gauge 25 6 40 0.4 0.5) 0)
(pin (measure-core "allocs-result"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (begin
                         (arena/allocs import-thunk)
                         nil)
                       (assign j (%add j 1)))) count-gauge 50 6 40 0.4 0.5) 0)
(delete-file import-module-path)
(delete-directory import-module-dir)

# ── Byte-gauge ────────────────────────────────────────────────────────
# Bump-arena bytes, not object count: a scope-dropped string must return its
# BYTES. Pinned as a range, shrink-only — catches a regression back to
# page-granular leaking.
(println "── folded suite: byte-gauge ──")
(pin (measure-core "string-bytes"
                   (fn [b]
                     (run-thunk-block (fn [j]
                                        (let [x (string "iter-" j
                                          "-padding-to-make-string-longer")]
                                          x)) b)) bytes-gauge 200 6 40 200.0
                   1000.0) [0 200])

# ── Value-survival correctness ────────────────────────────────────────
# Not rates — these assert a heap value SURVIVES rotation / resume, the
# correctness half of the suite the estimator does not cover.
(defn return-recur [n]
  (if (= n 0)
    (string "result-" n)
    (begin
      {:x n}
      (return-recur (%sub n 1)))))
(defn accum-recur [n acc]
  (if (= n 0) acc (accum-recur (%sub n 1) (%add acc n))))
(println "── folded suite: correctness pins ──")
(check (assert (= (return-recur 10000) "result-0")
               (string "return survives: " (return-recur 10000))))
(check (assert (= (accum-recur 10000 0) 50005000)
               (string "accumulator: " (accum-recur 10000 0))))
(check (let [fib (fiber/new (fn []
                              (def @i 0)
                              (while (%lt i 1000)
                                (yield (string "val-" i))
                                (assign i (%add i 1)))) |:yield|)
             vals (do
                    (def @acc @[])
                    (while (not= (fiber/status fib) :dead)
                      (push acc (fiber/resume fib)))
                    acc)]
         (assert (= (get vals 0) "val-0")
                 (string "yield-at-scale first: " (get vals 0)))
         (assert (= (get vals 999) "val-999")
                 (string "yield-at-scale last: " (get vals 999)))))
(check (assert (= (concat [1 2] [3 4]) [1 2 3 4]) "array concat value"))
(check (assert (= (concat "foo" "bar") "foobar") "string concat value"))
# The closure-as-module's accessor still reaches its captured value after the
# module's frame is gone. `module-cell-read-window` above prices the fallback the
# frame-exit relocation takes for this shape; this is the property that fallback
# exists to keep, and it is a value assertion because neither memory gauge can
# see it — the box release and the release routed through the box are correctly
# COUNTED either way, so an inverted pair reads flat here and clean under
# `--trace=guardfree`. The emission order itself is stated over the finished
# blocks by `lir::lower::assert_cells_outlive_their_readers`, which runs in every
# debug build over every block it lowers.
(check (assert (= ((get (mod-cell-immediate) :p)) (ptr/from-int 7))
               "closure-as-module accessor read back its Immediate-init capture"))
(check (assert (= ((get (mod-cell-heap) :p)) "cap")
               "closure-as-module accessor read back its heap-init capture"))
