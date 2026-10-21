(elle/epoch 15)
# audited: 2026-10-20
# std/framing answers the same lines whatever its source is and whatever chunk size it is read in.
# lib/framing.md
#
# The counter-factual: a framer that answered records from one chunk at a
# time reads a line that straddles two chunks as two lines. The chunk sizes
# below are chosen so that every line straddles a chunk edge at least once.

(def framing ((import "std/framing")))

## ── Helpers ───────────────────────────────────────────────────────────

(defn drain [f]
  "Every answer of f before its first nil, as strings, in order."
  (def out @[])
  (def @x (f))
  (while (not (nil? x))
    (push out (string x))
    (assign x (f)))
  (freeze out))

(defn with-port [path thunk]
  "Run (thunk p) over a binary read port on path, and close the port after."
  (let [p (port/open-bytes path :read)]
    (defer
      (port/close p)
      (thunk p))))

(defn framer-lines-of [path size]
  "The lines of the file at path through a framer of chunk size size."
  (with-port path
             (fn [p]
               (let [f (if size (framing:open p :size size) (framing:open p))]
                 (drain (fn [] (framing:line f)))))))

(defn port-lines-of [path]
  "The lines of the file at path through port/read-line."
  (with-port path (fn [p] (drain (fn [] (port/read-line p))))))

(defn chunked [data size]
  "A source answering data in chunks of size bytes, counting its calls."
  (def @pos 0)
  (def st @{:calls 0})
  (put st
       :read (fn [n]
               (put st :calls (inc st:calls))
               (if (>= pos (length data))
                 nil
                 (let [end (min (length data) (+ pos size))
                       chunk (slice data pos end)]
                   (assign pos end)
                   chunk))))
  st)

(defn generator [data size]
  "A fiber yielding data in chunks of size bytes, then ending."
  (fiber/new (fn []
               (def @pos 0)
               (while (< pos (length data))
                 (let [end (min (length data) (+ pos size))]
                   (yield (slice data pos end))
                   (assign pos end)))) |:yield|))

(defn lines-of [source size]
  "The lines a framer of chunk size size over source answers, as strings."
  (let [f (framing:open source :size size)]
    (drain (fn [] (framing:line f)))))

## ── Fixtures ──────────────────────────────────────────────────────────

(def fixture "one\ntwo\r\n\nmid\rline\r\n\r\nlast")
(def fixture-lines ["one" "two" "" "mid\rline" "" "last"])

(with-temp-dir dir
               (let [plain (path/join dir "plain")
                     cr (path/join dir "cr")]
                 (file/write plain fixture)
                 (file/write cr (string fixture "\r"))

                 ## ── Parity with port/read-line ──────────────────────────────────
                 (assert (= fixture-lines (port-lines-of plain))
                         "the fixture reads as six lines through port/read-line")
                 (assert (= (port-lines-of plain) (framer-lines-of plain nil))
                         "framing:line agrees with port/read-line on LF, CRLF, empty and lone-CR lines")
                 (assert (= (port-lines-of cr) (framer-lines-of cr nil))
                         "a partial last line keeps its trailing CR on both")
                 (assert (= "last\r" (get (framer-lines-of cr nil) 5))
                         "the partial last line is answered whole, CR included")
                 (println "  1. parity with port/read-line")

                 ## ── Chunk boundaries ────────────────────────────────────────────
                 (each size in [1 2 3 7 64]
                   (assert (= fixture-lines (framer-lines-of plain size))
                           (string "chunk size " size " answers the same lines"))
                   (assert (= (port-lines-of cr) (framer-lines-of cr size))
                           (string "chunk size " size
                                   " keeps the partial last line's CR")))
                 (println "  2. every chunk size answers the same lines")

                 ## ── Records are bytes ───────────────────────────────────────────
                 (with-port plain
                            (fn [p]
                              (let [f (framing:open p :size 2)
                                    l (framing:line f)]
                                (assert (bytes? l) "a record is bytes")
                                (assert (= (bytes "one") l)
                                        "equal to the bytes of its text")
                                (assert (= "one" (string l))
                                        "and its string form is the text"))))
                 (println "  3. a record is bytes")

                 ## ── Every source kind ───────────────────────────────────────────
                 (each size in [1 3 64]
                   (with-port plain
                              (fn [p]
                                (assert (= fixture-lines (lines-of p size))
                                        (string "a binary port, chunks of " size))))
                   (assert (= fixture-lines
                              (lines-of (get (chunked (bytes fixture) size)
                                        :read) size))
                           (string "a function of n, chunks of " size))
                   (assert (= fixture-lines
                              (lines-of (port/chunks (port/open-bytes plain
                                        :read) size) size))
                           (string "a port/chunks fiber, chunks of " size))
                   (assert (= fixture-lines
                              (lines-of (generator (bytes fixture) size) size))
                           (string "a generator fiber yielding bytes, chunks of "
                                   size))
                   (assert (= fixture-lines
                              (lines-of (generator fixture size) size))
                           (string "a generator fiber yielding strings, chunks of "
                                   size))
                   (assert (= fixture-lines (lines-of (bytes fixture) size))
                           (string "bytes in memory, asked for " size))
                   (assert (= fixture-lines (lines-of fixture size))
                           (string "a string in memory, asked for " size)))
                 (let [m (@bytes fixture)
                       f (framing:open m)
                       l (framing:line f)]
                   (assert (= (bytes "one") l)
                           "mutable bytes in memory are read as they are")
                   (assert (not (mutable? l)) "and the record is immutable"))
                 (println "  4. every source kind answers the same lines")

                 ## ── A text port is refused ──────────────────────────────────────
                 (let [p (port/open plain :read)
                       [ok? err] (protect (framing:open p))]
                   (port/close p)
                   (assert (not ok?)
                           "a text port is refused: the framer counts bytes")
                   (assert (= :framing-error (get err :error))
                           "as a framing error")
                   (assert (= :text-port (get err :reason))
                           "that names the reason"))
                 (println "  5. a text port is refused")))

## ── An empty source ───────────────────────────────────────────────────

(let [f (framing:open (fn [n] nil))]
  (assert (nil? (framing:line f)) "line on an empty source")
  (assert (nil? (framing:until f "\r\n")) "until on an empty source")
  (assert (nil? (framing:exact f 1)) "exact on an empty source")
  (assert (nil? (framing:up-to f 1)) "up-to on an empty source")
  (assert (empty? (stream/collect (framing:lines f))) "lines on an empty source"))
(let [f (framing:open "")]
  (assert (nil? (framing:line f)) "an empty string in memory is an empty source"))
(let [f (framing:open (fiber/new (fn [] nil) |:yield|))]
  (assert (nil? (framing:line f))
          "a fiber that yields nothing is an empty source"))
(let [[ok? err] (protect (framing:open 42))]
  (assert (not ok?) "a source that is none of the four kinds is refused")
  (assert (= :framing-error (get err :error)) "as a framing error")
  (assert (= :bad-source (get err :reason)) "that names the reason"))
(println "  6. an empty source answers nil")

(println "framing: ok")
