(elle/epoch 12)
# audited: 2026-09-23
## tests/elle/hash.lisp — lib/hash.lisp: what std/hash reads from a file or a
## port, and what it leaves behind.
##
## The module takes its plugin as an argument, so `echo` stands in for the
## hash plugin: its digest is the list of chunks it was fed. The tests see
## exactly the bytes std/hash read, and run where no plugin is built.
##
## The counter-factual: a digest compared against a known hash would pass as
## long as the plugin received the right bytes by any route. An echo digest also
## fails when a read error comes back as a chunk instead of raising.

(def echo
  {:new (fn [_algorithm] @[])
   :update (fn [ctx chunk]
             (push ctx chunk)
             ctx)
   :finalize (fn [ctx] (freeze ctx))})

(def h ((import "std/hash") echo))

(defn joined [chunks]
  "The bytes of every chunk in order, as one bytes value."
  (each c in chunks
    (assert (or (bytes? c) (string? c))
            "every chunk the plugin receives is data, not an error value"))
  (if (empty? chunks) (bytes) (apply concat (map bytes chunks))))

(defn write-bytes [path data]
  "Write data to the file at path through a binary port."
  (let [p (port/open-bytes path :write)]
    (port/write p data)
    (port/close p)))

## Every byte value, so no chunk of it is valid UTF-8 by accident.
(def every-byte (apply bytes (range 256)))

## ── hash/file reads the bytes, whatever they hold ──────────────────

(with-temp-dir dir
               (let [path (path/join dir "binary")]
                 (write-bytes path every-byte)
                 (let [[ok? chunks] (protect (h:file :sha256 path))]
                   (assert ok? "hash/file hashes a file that is not UTF-8")
                   (assert (= (bytes->hex (joined chunks))
                              (bytes->hex every-byte))
                           "hash/file feeds the plugin every byte of the file, in order"))))

## A text port cuts a read at a byte count and then decodes it, so a chunk
## boundary inside `é` fails on a file that is valid UTF-8.
(with-temp-dir dir
               (let [path (path/join dir "text")]
                 (file/write path "héllo")
                 (let [[ok? chunks] (protect (h:file :sha256 path :chunk-size 2))]
                   (assert ok?
                           "hash/file hashes UTF-8 whose character straddles a chunk boundary")
                   (assert (= (bytes->hex (joined chunks))
                              (bytes->hex (bytes "héllo")))
                           "hash/file feeds the plugin the file's bytes across the boundary"))))

(with-temp-dir dir
               (let [path (path/join dir "empty")]
                 (write-bytes path (bytes))
                 (let [[ok? chunks] (protect (h:file :sha256 path))]
                   (assert ok? "hash/file hashes an empty file")
                   (assert (empty? chunks)
                           "an empty file feeds the plugin no chunk"))))

(with-temp-dir dir
               (let [path (path/join dir "binary")]
                 (write-bytes path every-byte)
                 (let [[ok? chunks] (protect (h:file :sha256 path
                       :chunk-size 100))]
                   (assert ok? "hash/file honors :chunk-size")
                   (assert (all? (fn [c] (<= (length c) 100)) chunks)
                           "no chunk is longer than :chunk-size")
                   (assert (= (bytes->hex (joined chunks))
                              (bytes->hex every-byte))
                           "the chunks still carry every byte"))))

## ── hash/digest reads from the port and leaves it open ─────────────

(with-temp-dir dir
               (let [path (path/join dir "binary")]
                 (write-bytes path every-byte)
                 (let [p (port/open-bytes path :read)]
                   (defer
                     (port/close p)
                     (port/read p 16)
                     (let [chunks (h:digest :sha256 p)]
                       (assert (= (bytes->hex (joined chunks))
                                  (bytes->hex (slice every-byte 16 256)))
                               "hash/digest hashes what remains to be read from the port")
                       (assert (port/open? p) "hash/digest leaves the port open"))))))

## A read that fails must raise in the caller. A digest of the chunks read
## before the failure is a wrong answer that looks right.
(with-temp-dir dir
               (let [path (path/join dir "binary")]
                 (write-bytes path every-byte)
                 (let [p (port/open path :read)]
                   (defer
                     (port/close p)
                     (let [[ok? err] (protect (h:digest :sha256 p))]
                       (assert (not ok?) "a read error in hash/digest raises")
                       (assert (= (get err :error) :encoding-error)
                               "the raised error is the read's own"))))))

(println "hash: all tests passed")
