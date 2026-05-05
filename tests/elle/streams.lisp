(elle/epoch 10)
# Stream combinators — sinks, transforms, port-to-stream converters


# === Helpers ===

(defn make-range [n]
  "Return a fiber that yields integers 0..n-1."
  (fiber/new (fn []
               (def @i 0)
               (while (< i n)
                 (yield i)
                 (assign i (+ i 1)))) |:yield|))

(defn make-from-list [lst]
  "Return a fiber that yields each element of lst."
  (fiber/new (fn []
               (def @remaining lst)
               (while (not (empty? remaining))
                 (yield (first remaining))
                 (assign remaining (rest remaining)))) |:yield|))

# === Sink combinators ===

# stream/collect: finite fiber
(assert (= (stream/collect (make-from-list (list 1 2 3))) (list 1 2 3))
        "stream/collect: three values in order")

# stream/collect: empty fiber (immediately done)
(assert (= (stream/collect (make-range 0)) ())
        "stream/collect: empty source yields empty list")

# stream/fold: sum
(assert (= (stream/fold + 0 (make-range 5)) 10) "stream/fold: sum 0..4 = 10")

# stream/fold: initial value returned on empty source
(assert (= (stream/fold + 99 (make-range 0)) 99)
        "stream/fold: empty source returns init")

# stream/for-each: side effects accumulate into mutable array
(let [acc @[]]
  (stream/for-each (fn [v] (push acc v)) (make-range 4))
  (assert (= (length acc) 4) "stream/for-each: correct element count")
  (assert (= (get acc 0) 0) "stream/for-each: first element")
  (assert (= (get acc 3) 3) "stream/for-each: last element"))

# stream/for-each: returns nil
(assert (= (stream/for-each (fn [v] v) (make-range 3)) nil)
        "stream/for-each: returns nil")

# stream/into-array: basic
(let [result (stream/into-array (make-from-list (list 10 20 30)))]
  (assert (= (length result) 3) "stream/into-array: length")
  (assert (= (get result 0) 10) "stream/into-array: first element")
  (assert (= (get result 2) 30) "stream/into-array: last element"))

# stream/into-array: empty source
(assert (= (length (stream/into-array (make-range 0))) 0)
        "stream/into-array: empty source yields empty array")

# === Transform combinators ===

# stream/map: identity transform
(assert (= (stream/collect (stream/map identity (make-range 3))) (list 0 1 2))
        "stream/map: identity preserves values")

# stream/map: squaring transform
(assert (= (stream/collect (stream/map (fn [x] (* x x)) (make-range 4)))
           (list 0 1 4 9)) "stream/map: squares 0..3")

# stream/map: empty source
(assert (= (stream/collect (stream/map (fn [x] (* x 10)) (make-range 0))) ())
        "stream/map: empty source yields empty")

# stream/filter: keeps matching values
(assert (= (stream/collect (stream/filter (fn [x] (= (rem x 2) 0))
                           (make-range 6))) (list 0 2 4))
        "stream/filter: keeps even values")

# stream/filter: rejects all
(assert (= (stream/collect (stream/filter (fn [x] false) (make-range 3))) ())
        "stream/filter: all rejected yields empty")

# stream/filter: keeps all
(assert (= (stream/collect (stream/filter (fn [x] true) (make-range 3)))
           (list 0 1 2)) "stream/filter: all kept")

# stream/take: fewer than available
(assert (= (stream/collect (stream/take 3 (make-range 10))) (list 0 1 2))
        "stream/take: take 3 from 10")

# stream/take: more than available
(assert (= (stream/collect (stream/take 10 (make-range 3))) (list 0 1 2))
        "stream/take: take 10 from 3 yields all 3")

# stream/take: zero
(assert (= (stream/collect (stream/take 0 (make-range 5))) ())
        "stream/take: take 0 yields empty")

# stream/drop: fewer than available
(assert (= (stream/collect (stream/drop 2 (make-range 5))) (list 2 3 4))
        "stream/drop: drop 2 from 5")

# stream/drop: more than available
(assert (= (stream/collect (stream/drop 10 (make-range 3))) ())
        "stream/drop: drop more than available yields empty")

# stream/drop: zero
(assert (= (stream/collect (stream/drop 0 (make-range 3))) (list 0 1 2))
        "stream/drop: drop 0 yields all")

# stream/concat: two non-empty sources
(assert (= (stream/collect (stream/concat (make-from-list (list 1 2))
                           (make-from-list (list 3 4)))) (list 1 2 3 4))
        "stream/concat: two sources concatenated")

# stream/concat: empty source in the middle
(assert (= (stream/collect (stream/concat (make-from-list (list 1))
                           (make-range 0) (make-from-list (list 2)))) (list 1 2))
        "stream/concat: empty source in middle is skipped")

# stream/concat: dead (pre-exhausted) fiber as first argument
# The dead fiber must be skipped gracefully, no error.
(let [dead (make-range 2)]
  (stream/collect dead)
  (assert (= (stream/collect (stream/concat dead (make-from-list (list 99))))
             (list 99)) "stream/concat: dead first source skipped"))

# stream/zip: same-length sources
(assert (= (stream/collect (stream/zip (make-from-list (list 1 2 3))
                                       (make-from-list (list 4 5 6))))
           (list [1 4] [2 5] [3 6])) "stream/zip: same-length sources")

# stream/zip: stops at shortest (first exhausted)
(assert (= (stream/collect (stream/zip (make-range 2) (make-range 5)))
           (list [0 0] [1 1])) "stream/zip: stops at shortest source")

# stream/zip: one empty source yields empty immediately
(assert (= (stream/collect (stream/zip (make-range 0) (make-range 3))) ())
        "stream/zip: empty source causes immediate stop")

# stream/pipe: single transform
(assert (= (stream/collect (stream/pipe (make-range 3)
                                        (partial stream/map (fn [x] (* x 2)))))
           (list 0 2 4)) "stream/pipe: single transform")

# stream/pipe: chained transforms
(assert (= (stream/collect (stream/pipe (make-range 10)
                                        (partial stream/filter
                                        (fn [x] (= (rem x 2) 0)))
                                        (partial stream/take 3))) (list 0 2 4))
        "stream/pipe: filter then take")

# Composition: map then filter then take
(assert (= (stream/collect (stream/take 2
                                        (stream/filter (fn [x] (> x 3))
                                        (stream/map (fn [x] (* x 2))
                                        (make-range 10))))) (list 4 6))
        "composition: map*2 then filter >3 then take 2 from range 0..9")

# === Port-to-stream converters ===
# User code runs inside the async scheduler, so I/O works directly.

# port/lines: multi-line file
(spit "/tmp/elle-test-streams-lines-478" "alpha\nbeta\ngamma")
(assert (= (stream/collect (port/lines (port/open "/tmp/elle-test-streams-lines-478"
                                       :read))) (list "alpha" "beta" "gamma"))
        "port/lines: yields all lines from multi-line file")

# port/lines: empty file yields empty list
(spit "/tmp/elle-test-streams-empty-478" "")
(assert (= (stream/collect (port/lines (port/open "/tmp/elle-test-streams-empty-478"
                                       :read))) ())
        "port/lines: empty file yields empty list")

# port/lines: file without trailing newline
(spit "/tmp/elle-test-streams-nonl-478" "no-newline")
(assert (= (stream/collect (port/lines (port/open "/tmp/elle-test-streams-nonl-478"
                                       :read))) (list "no-newline"))
        "port/lines: file without trailing newline yields last line")

# port/lines: port is closed after collect exhausts the stream
(spit "/tmp/elle-test-streams-close-478" "one\ntwo")
(let [p (port/open "/tmp/elle-test-streams-close-478" :read)]
  (stream/collect (port/lines p))
  (assert (not (port/open? p))
          "port/lines: port is closed after stream is exhausted"))

# port/chunks: basic chunking, 4-byte chunks of 12-byte file
(spit "/tmp/elle-test-streams-chunks-478" "abcdefghijkl")
(assert (= (stream/collect (port/chunks (port/open "/tmp/elle-test-streams-chunks-478"
                                        :read) 4)) (list "abcd" "efgh" "ijkl"))
        "port/chunks: 12-byte file in 4-byte chunks")

# port/chunks: remainder chunk — 10-byte file with 4-byte chunks yields [4, 4, 2]
(spit "/tmp/elle-test-streams-chunks2-478" "abcdefghij")
(let [result (stream/collect (port/chunks (port/open "/tmp/elle-test-streams-chunks2-478"
                             :read) 4))]
  (assert (= (length result) 3) "port/chunks: remainder — 3 chunks")
  (assert (= (get result 0) "abcd") "port/chunks: remainder — first chunk")
  (assert (= (get result 1) "efgh") "port/chunks: remainder — second chunk")
  (assert (= (get result 2) "ij") "port/chunks: remainder — final short chunk"))

# port/chunks: port is closed after stream is exhausted
(spit "/tmp/elle-test-streams-chunkclose-478" "hello")
(let [p (port/open "/tmp/elle-test-streams-chunkclose-478" :read)]
  (stream/collect (port/chunks p 3))
  (assert (not (port/open? p))
          "port/chunks: port is closed after stream is exhausted"))

# port/writer: write values and read back
(let* [p (port/open "/tmp/elle-test-streams-writer-478" :write)
       w (port/writer p)]
  (fiber/resume w)  # start: advance to first yield nil
  (fiber/resume w "hello ")  # write "hello "
  (fiber/resume w "world")  # write "world"
  (fiber/resume w nil))  # nil signals close
(assert (= (slurp "/tmp/elle-test-streams-writer-478") "hello world")
        "port/writer: writes values to port")

# port/writer: port is closed after nil resume
(let* [p (port/open "/tmp/elle-test-streams-writerclose-478" :write)
       w (port/writer p)]
  (fiber/resume w)  # start
  (fiber/resume w "data")  # write
  (fiber/resume w nil))
# close

# Composition: port/lines -> stream/map -> stream/take -> stream/collect
(spit "/tmp/elle-test-streams-compose-478" "1\n2\n3\n4\n5")
(assert (= (stream/collect (stream/take 3
                                        (stream/map (fn [x] (+ (parse-int x) 10))
                                        (port/lines (port/open "/tmp/elle-test-streams-compose-478"
                                        :read))))) (list 11 12 13))
        "composition: port/lines -> map -> take -> collect")

# === Integration tests ===

# Full pipeline: file -> lines -> drop header -> map parse -> filter -> take -> collect
(spit "/tmp/elle-test-streams-pipeline-478"
      "id,value\n1,100\n2,200\n3,300\n4,400\n5,500")
(assert (= (stream/collect (stream/take 3
                                        (stream/filter (fn [row]
                                          (> (get row :value) 150))
                                        (stream/map (fn [line]
                                          (let [parts (string/split line ",")]
                                            {:id (parse-int (get parts 0))
                                            :value (parse-int (get parts 1))}))
                                        (stream/drop 1
                                        (port/lines (port/open "/tmp/elle-test-streams-pipeline-478"
                                        :read)))))))
           (list {:id 2 :value 200} {:id 3 :value 300} {:id 4 :value 400}))
        "integration: CSV pipeline drop-header map-parse filter take collect")

# Nested streams: stream/concat of stream/map-ped sources
(assert (= (stream/collect (stream/concat (stream/map (fn [x] (* x 10))
                           (make-from-list (list 1 2)))
                           (stream/map (fn [x] (* x 100))
                                       (make-from-list (list 3 4)))))
           (list 10 20 300 400)) "nested streams: concat of mapped sources")

# === Error propagation ===

# stream/for-each with a callback that errors — error propagates to caller
(let [[ok? _] (protect ((fn []
                          (stream/for-each (fn [v]
                            (when (= v 2)
                              (error {:error :test-error :message "stop at 2"})))
                          (make-range 5)))))]
  (assert (not ok?) "stream/for-each: callback error propagates"))

# stream/fold with a callback that errors — error propagates
(let [[ok? _] (protect ((fn []
                          (stream/fold (fn [acc v]
                                         (when (= v 3)
                                           (error {:error :test-error
                                           :message "stop at 3"}))
                                         (+ acc v)) 0 (make-range 5)))))]
  (assert (not ok?) "stream/fold: callback error propagates"))

# stream/map with a transform that errors — error propagates through collect
(let [[ok? _] (protect ((fn []
                          (stream/collect (stream/map (fn [v]
                            (when (= v 1)
                              (error {:error :test-error :message "stop at 1"}))
                            v) (make-range 3))))))]
  (assert (not ok?) "stream/map: transform error propagates through collect"))

# Closed port: port/lines yields the io-error as a value (not signaled)
(spit "/tmp/elle-test-streams-closederror-478" "some data")
(let [[ok? val] (protect ((fn []
                            (let [p (port/open "/tmp/elle-test-streams-closederror-478"
                                  :read)]
                              (port/close p)
                              (stream/collect (port/lines p))))))]
  (assert ok? "closed port: stream/collect succeeds (error yielded as value)")
  (assert (= (get (first val) :error) :io-error)
          "closed port: collected element is io-error"))
