(elle/epoch 16)
# audited: 2026-11-10
# A store into a module's own typed array claims no page, as the same store claims none in a file.
# docs/pipeline.md
# docs/impl/region/diagnostics.md
#
# The counter-factual: the loaders compiled a module's form through eval, which
# lowered with no types. Each put into the array was then the general call, and
# each call claimed a page, so the window below read one page per store.

(defn window [n]
  "The pages claimed while N stores land in an array this function made."
  (when (%not (%int? n)) (error :n))
  (def b @[])
  (push b 0)
  (def @i 0)
  (let [before (arena/page-claims)]
    (while (%lt i n)
      (put b 0 i)
      (assign i (%add i 1)))
    (- (arena/page-claims) before)))

(def loaded
  (import/load-source "<window>"
                      "(fn [n]
                         (when (%not (%int? n)) (error :n))
                         (def b @[])
                         (push b 0)
                         (def @i 0)
                         (let [before (arena/page-claims)]
                           (while (%lt i n)
                             (put b 0 i)
                             (assign i (%add i 1)))
                           (- (arena/page-claims) before)))"))

(window 400)
(loaded 400)
(assert (= 0 (window 400)) "the stores in a file claim no page")
(assert (= 0 (loaded 400)) "nor do the same stores in a module")
