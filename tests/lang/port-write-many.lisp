(elle/epoch 15)
# audited: 2026-11-11
## port/write-many sends every byte of every part, in order, in one call, and returns the bytes it sent.
## docs/io/write.md
##
## The parts reach the peer as one stream, so every case compares what the
## peer read with the parts joined, and the count with their size in bytes.
## The two fail independently: a write that stops after the first part's
## short write keeps a plausible count only if it lies about it, and one that
## restarts a part from its first byte after a short write sends the right
## number of bytes in the wrong order.
##
## A 4 KiB send buffer takes about 21 KB of a write at once, so a 200 KB
## message in seven parts is cut inside a part several times. A message of
## 3000 parts is more than one gathered write can carry (IOV_MAX is 1024 on
## Linux), so it goes out as several writes inside the one call.
##
## The thread-pool build runs this file too, so both backends answer it.

(defn listen-port [listener]
  "The port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn joined [parts]
  "Every byte of parts, joined in order, as bytes."
  (def @acc (@bytes))
  (each p in parts
    (append acc (bytes p)))
  (freeze acc))

(defn size-of [parts]
  "The bytes in parts, summed."
  (fold (fn [n p] (+ n (length (bytes p)))) 0 parts))

(defn through-socket [parts sndbuf]
  "Write parts with one port/write-many over a loopback connection whose send
   buffer is sndbuf bytes, or the default for 0. Returns [count received]:
   what the call answered, and every byte the peer read before the end."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         reader (ev/spawn (fn []
                            (let [conn (tcp/accept listener)
                                  got @[]]
                              (forever
                                (let [chunk (port/read conn 65536)]
                                  (when (nil? chunk) (break))
                                  (push got chunk)))
                              (port/close conn)
                              (joined got))))
         conn (if (> sndbuf 0)
                (tcp/connect "127.0.0.1" (listen-port listener) :sndbuf sndbuf
                             :timeout 5)
                (tcp/connect "127.0.0.1" (listen-port listener) :timeout 5))
         count (port/write-many conn parts)]
    (port/close conn)
    (let [received (ev/join reader)]
      (port/close listener)
      [count received])))

## ── 1. The parts arrive in order, and the count is their bytes ────────

(with-temp-dir dir
  (let [path (path/join dir "parts")
        w (port/open path :write)]
    (assert (= 6 (port/write-many w ["é" (bytes 0 1) "" @"ab"]))
            "the count is bytes, not characters: é is two")
    (assert (= 4 (port/write-many w (list @b[1 2] "xy")))
            "a list of mutable and immutable parts")
    (assert (= 3 (port/write-many w @["a" (bytes 98) @"c"]))
            "parts held in an @array")
    (assert (= 0 (port/write-many w ["" (bytes) @""]))
            "parts that are all empty write nothing")
    (assert (= 0 (port/write-many w (list))) "an empty list writes nothing")
    (port/close w)
    (assert (= (joined ["é" (bytes 0 1) "ab" (bytes 1 2) "xy" "abc"])
               (bytes (file/read path))) "every part, in order")))
(println "  1. parts arrive in order and the count is their bytes")

## ── 2. A refused call writes nothing ──────────────────────────────────

(with-temp-dir dir
  (let [path (path/join dir "refused")
        w (port/open path :write)
        refused (fn [parts]
                  (let [[ok? err] (protect (port/write-many w parts))]
                    (and (not ok?) (= :type-error (get err :error)))))]
    (assert (refused ["a" 42]) "a number is not a part")
    (assert (refused ["a" :b]) "a keyword is not a part")
    (assert (refused ["a" ["b"]]) "a part does not nest")
    (assert (refused "a") "a string is not a sequence of parts")
    (assert (refused (bytes 1)) "bytes are not a sequence of parts")
    (assert (refused {:a "b"}) "a struct is not a sequence of parts")
    (assert (refused nil) "nil is not a sequence of parts")
    (port/close w)
    (assert (= "" (file/read path))
            "a part refused after a good one still writes no byte")))
(println "  2. a refused call raises a type-error and writes nothing")

## ── 3. A message cut inside its parts by a small send buffer ──────────

(def message
  [(string/repeat "a" 70000) (bytes (string/repeat "b" 3)) ""
   (string/repeat "c" 50000) (thaw (bytes (string/repeat "d" 30001))) @"é"
   (bytes (string/repeat "e" 49994))])

(let [[count received] (through-socket message 4096)]
  (assert (= count (size-of message))
          (string "a 4 KiB send buffer: the call answered " count " of "
                  (size-of message) " bytes"))
  (assert (= (length received) (size-of message))
          (string "a 4 KiB send buffer: the peer read " (length received) " of "
                  (size-of message) " bytes"))
  (assert (= received (joined message))
          "a 4 KiB send buffer: the peer read the parts out of order"))
(println "  3. 200 KB in seven parts through a 4 KiB send buffer")

## ── 4. More parts than one gathered write carries ─────────────────────

(def many
  (map (fn [i] (if (even? i) (string "<" i ">") (bytes (string "[" i "]"))))
       (range 3000)))

(let [[count received] (through-socket many 4096)]
  (assert (= count (size-of many))
          (string "3000 parts: the call answered " count " of " (size-of many)
                  " bytes"))
  (assert (= received (joined many))
          "3000 parts: the peer read them out of order or short"))
(println "  4. 3000 parts in one call")

## ── 5. A peer that stops reading times the call out ───────────────────
##
## The peer accepts and never reads, so after the send buffer fills every
## later write waits. The call must give up at its deadline with :timeout.

(let* [listener (tcp/listen "127.0.0.1" 0)
       @peer nil
       _ (ev/spawn (fn [] (assign peer (tcp/accept listener))))
       conn (tcp/connect "127.0.0.1" (listen-port listener) :sndbuf 4096
                         :timeout 5)
       chunk (bytes (string/repeat "x" 1000000))
       started (clock/monotonic)
       [ok? err] (protect (port/write-many conn [chunk chunk chunk chunk]
                            :timeout 0.5))
       elapsed (- (clock/monotonic) started)]
  (assert (not ok?) "a write to a peer that never reads must not succeed")
  (assert (= :timeout (get err :error))
          (string "expected a :timeout error, got " err))
  (assert (< elapsed 10)
          (string "the call ran " elapsed "s past a :timeout of 0.5s"))
  (port/close conn)
  (when peer (port/close peer))
  (port/close listener))
(println "  5. :timeout bounds a call whose peer stops reading")

(println "port-write-many: all tests passed")
