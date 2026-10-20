(elle/epoch 15)
# audited: 2026-10-20
# std/framing answers the same records whatever its source is and whatever chunk size it is read in.
# lib/framing.md
#
# The counter-factual: a framer that answered records from one chunk at a
# time reads a line that straddles two chunks as two lines. The chunk sizes
# below are chosen so that every line, every delimiter and every exact count
# straddles a chunk edge at least once.

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

(defn strings-of [coll]
  "The string form of each element of coll, as an array."
  (def out @[])
  (each x in coll
    (push out (string x)))
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

(defn until-all [data delim size]
  "Every record of data before delim, read through chunks of size bytes."
  (let [src (chunked (bytes data) size)
        f (framing:open src:read)]
    (drain (fn [] (framing:until f delim)))))

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

## ── until ─────────────────────────────────────────────────────────────

(each size in [1 2 3 4 64]
  (assert (= ["ab" "cd" "" "ef"] (until-all "ab\r\ncd\r\n\r\nef" "\r\n" size))
          (string "a CRLF delimiter straddling chunks of " size))
  (assert (= ["ab" "cd" "" "ef"]
             (until-all "ab\r\ncd\r\n\r\nef" (bytes "\r\n") size))
          (string "the delimiter as bytes, chunks of " size))
  (assert (= ["x" "yy" "" "z"] (until-all "xENDyyENDENDz" "END" size))
          (string "a three-byte delimiter straddling chunks of " size))
  (assert (= ["a" "b" ""] (until-all "a\nb\n\n" 10 size))
          (string "an integer delimiter, chunks of " size)))

(let [src (chunked (bytes "abc") 2)
      f (framing:open src:read)]
  (assert (= (bytes "abc") (framing:until f "XY"))
          "a delimiter never found answers the rest")
  (assert (nil? (framing:until f "XY")) "and then nil")
  (assert (nil? (framing:until f "XY")) "and nil again"))
(let [f (framing:open ";;")]
  (assert (= (bytes) (framing:until f ";")) "an empty record is empty bytes")
  (assert (= (bytes) (framing:until f ";")) "and so is the next")
  (assert (nil? (framing:until f ";")) "nil only at the end"))
(println "  6. until finds a delimiter across any chunk edge")

## ── exact ─────────────────────────────────────────────────────────────

(defn exact-shapes [f]
  "The exact reads of the twenty-byte fixture, through a framer of chunk size 8."
  (assert (= (bytes "012") (framing:exact f 3)) "exact inside a chunk")
  (assert (= (bytes "34567") (framing:exact f 5)) "exact up to a chunk's end")
  (assert (= (bytes "89abcdefgh") (framing:exact f 10)) "exact across chunks")
  (assert (nil? (framing:exact f 5)) "exact past the end answers nil")
  (assert (nil? (framing:exact f 1)) "and the bytes it held went with it"))

(def twenty "0123456789abcdefghij")
(with-temp-dir dir
               (let [path (path/join dir "twenty")]
                 (file/write path twenty)
                 (with-port path
                            (fn [p] (exact-shapes (framing:open p :size 8))))
                 (exact-shapes (framing:open (port/chunks (port/open-bytes path
                               :read) 8) :size 8))))
(exact-shapes (framing:open (get (chunked (bytes twenty) 8) :read)))
(exact-shapes (framing:open (generator (bytes twenty) 8)))
(println "  7. exact answers the count or nil")

## ── up-to and rest ────────────────────────────────────────────────────

(let [src (chunked (bytes "abcdefgh") 3)
      f (framing:open src:read)]
  (assert (= (bytes "abc") (framing:up-to f 10))
          "up-to answers what one fill holds, not more")
  (assert (= (bytes "d") (framing:up-to f 1)) "up-to answers less than it holds")
  (assert (= (bytes "ef") (framing:up-to f 10)) "and the rest of the chunk")
  (assert (= (bytes "gh") (framing:up-to f 10)) "then fills once more")
  (assert (nil? (framing:up-to f 10)) "nil at the end")
  (assert (= 4 src:calls) "three chunks and the end: one call each"))
(let [src (chunked (bytes "abcdefgh") 3)
      f (framing:open src:read)]
  (assert (= (bytes "ab") (framing:exact f 2)) "two bytes of the first chunk")
  (assert (= (bytes "cdefgh") (framing:rest f))
          "rest joins the held byte and every chunk after")
  (assert (= (bytes) (framing:rest f)) "empty bytes once nothing is left")
  (assert (nil? (framing:up-to f 1)) "and up-to answers nil")
  (assert (nil? (framing:line f)) "and so does line"))
(let [f (framing:open (fn [n] nil))]
  (assert (= (bytes) (framing:rest f)) "rest of an empty source is empty bytes"))
(println "  8. up-to and rest")

## ── lines and records ─────────────────────────────────────────────────

(let [src (chunked (bytes "one\ntwo\r\nthree") 4)
      f (framing:open src:read)]
  (assert (= ["one" "two" "three"]
             (strings-of (stream/collect (framing:lines f))))
          "lines yields each line and ends at EOF"))
(let [src (chunked (bytes "xENDyyENDENDz") 4)
      f (framing:open src:read)]
  (assert (= ["x" "yy" "" "z"]
             (strings-of (stream/collect (framing:records f "END"))))
          "records yields each until record and ends at EOF"))
(let [f (framing:open "1\n2\n3\n4\n")
      doubled (stream/map (fn [l] (* 2 (int (string l)))) (framing:lines f))]
  (assert (= (list 2 4 6) (stream/collect (stream/take 3 doubled)))
          "lines composes with stream/map and stream/take"))
(with-temp-dir dir
               (let [path (path/join dir "lines")]
                 (file/write path "a\nb\n")
                 (with-port path
                            (fn [p]
                              (let [f (framing:open p)]
                                (assert (= ["a" "b"]
                                        (strings-of (stream/collect (framing:lines f))))
                                        "lines over a port")
                                (assert (port? p) "the port is still a port")
                                (assert (nil? (port/read p 1))
                                        "and still open, at its end"))))))
(println "  9. lines and records are streams")

## ── One chunk, one call ───────────────────────────────────────────────

(let [src (chunked (bytes "a\nb\nc\nd\ne\n") 100)
      f (framing:open src:read)]
  (assert (= ["a" "b" "c" "d" "e"] (drain (fn [] (framing:line f))))
          "five lines from one chunk")
  (assert (= 2 src:calls) "one call for the chunk and one for the end"))
(println " 10. a chunk of lines is one call on the source")

## ── The HTTP shape ────────────────────────────────────────────────────

(let [src (chunked (bytes "HEAD1\r\nHEAD2\r\n\r\nBODYBODYNEXT\r\n") 5)
      f (framing:open src:read)]
  (assert (= "HEAD1" (string (framing:line f))) "first header")
  (assert (= "HEAD2" (string (framing:line f))) "second header")
  (assert (= "" (string (framing:line f))) "the empty line ends the headers")
  (assert (= (bytes "BODYBODY") (framing:exact f 8)) "the body by its length")
  (assert (= "NEXT" (string (framing:line f))) "lines continue after the body")
  (assert (nil? (framing:line f)) "and end"))
(println " 11. lines, then exact, then lines")

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
(println " 12. an empty source answers nil")

(println "framing: ok")
