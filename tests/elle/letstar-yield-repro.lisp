#!/usr/bin/env elle
(elle/epoch 9)

# Regression test: let* + yield + calls that store heap objects externally.
#
# Root cause: let* desugars to nested single-binding let forms. The
# innermost let (whose body doesn't suspend) could be scope-allocated
# with RegionEnter/RegionExit. If the body calls a function that
# internally allocates heap objects and stores them in an external
# mutable structure (via put), RegionExit frees those objects while
# they're still referenced — use-after-free.

(defn make-table []
  @{})

(defn store-in-table [table key value]
  "Create a heap struct and store it in an external table."
  (put table key @{:data value :ts (clock/monotonic)}))

(defn timed-op [table thunk &named attributes]
  "let* where second binding yields, body stores heap objects externally."
  (let* [start (clock/monotonic)
         result (thunk)
         elapsed (- (clock/monotonic) start)]
    (store-in-table table "latest" elapsed)
    result))

(def @table (make-table))
(def @errors @[])

# Background fiber that reads the table
(def reader
  (ev/spawn (fn []
              (def @checks 0)
              (while (< checks 200)
                (ev/sleep 0.001)
                (let [entry (get table "latest")]
                  (when entry
                    (let [[ok? err] (protect (get entry :data))]
                      (unless ok?
                        (push errors err)
                        (break)))))
                (assign checks (+ checks 1))))))

# Run many timed operations
(def @i 0)
(while (< i 50)
  (timed-op table
            (fn []
              (ev/sleep 0.001)
              :done)
            :attributes {:method "GET"})
  (assign i (+ i 1)))

(ev/sleep 0.05)

(if (empty? errors)
  (println "PASS: let* + yield + external heap store")
  (begin
    (println "FAIL: use-after-free detected:")
    (each e in (freeze errors)
      (println "  " e))
    (exit 1)))
