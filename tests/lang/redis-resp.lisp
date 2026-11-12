(elle/epoch 15)
# audited: 2026-11-12
# The RESP client with no server: a reply across chunks, a command's parts, commands over files, and the codec's self-tests.
# docs/stdlib/redis.md

(def redis ((import-file "lib/redis.lisp")))

# ── A reply across the framer's chunks (no Redis needed) ────────────────────
#
# A connection reads replies through std/framing, so a bulk string longer
# than one chunk is one reply however the chunks fall, and the reply after
# it starts where the bulk ended. The counter-factual is a client that read
# the length line and the bulk through separate port reads and parsed the
# bytes between them as the next reply.

(with-temp-dir dir
  (let [path (path/join dir "bulk")
        payload (string/repeat "0123456789" 2000)]
    (file/write path (string "$20000\r\n" payload "\r\n+OK\r\n"))
    (let [c (redis:connection (port/open-bytes path :read) :size 4096)]
      (assert (= payload (redis:resp-read c))
              "a 20000-byte bulk string read through 4096-byte chunks")
      (assert (= "OK" (redis:resp-read c)) "and the reply after it")
      (let [[ok? err] (protect (redis:resp-read c))]
        (assert (not ok?) "the end of the stream is an error")
        (assert (= :unexpected-eof (get err :reason)) "named unexpected-eof"))
      (redis:close c))))
(println "  a bulk string across chunks: ok")

# ── A command's parts (no Redis needed) ─────────────────────────────────────
#
# A command is its parts, each argument as it is, so port/write-many sends a
# large value from where it lies. The counter-factual is an encoder that joins
# the parts into one string: it copies the value, and it reads bytes as text,
# so the byte 255 never reaches the wire.

(let [value (bytes 255 0 1)
      parts (redis:resp-encode "SET" "k" value)]
  (assert (= :array (type-of parts)) "the parts are an immutable array")
  (assert (= ["*3\r\n" "$3\r\n" "SET" "\r\n" "$1\r\n" "k" "\r\n" "$3\r\n" value
              "\r\n"] parts) "a bytes argument is one part, as it is"))
(assert (= "$5\r\n" (get (redis:resp-encode "SET" "k" "café") 7))
        "a string's length counts its bytes")
(assert (= "60" (get (redis:resp-encode "EXPIRE" "k" 60) 8))
        "any other argument is its string text")
(println "  a command's parts: ok")

# ── Commands over a connection the caller built (no Redis needed) ───────────
#
# The connection writes to one file and reads its replies from another, so the
# file holds exactly what went on the wire. The second redis:using finds the
# connection open, because using never closes it.

(defn read-bytes [path]
  (let [p (port/open-bytes path :read)]
    (defer
      (port/close p)
      (port/read-all p))))

(with-temp-dir dir
  (let [replies (path/join dir "replies")
        sent (path/join dir "sent")
        value (bytes 255 0 1)]
    (file/write replies "+OK\r\n$1\r\na\r\n$1\r\nb\r\n")
    (let [in (port/open-bytes replies :read)
          out (port/open-bytes sent :write)
          c (merge (redis:connection in) {:port out})]
      (redis:using c
                   (fn []
                     (assert (redis:set "k" value) "the reply to a command")))
      (redis:using c
                   (fn []
                     (assert (= ["a" "b"]
                                (redis:pipeline (list "GET" "a")
                                  (list "GET" "b")))
                             "a pipeline's replies, in order")))
      (port/close in)
      (port/close out))
    (assert (= (concat (bytes "*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$3\r\n") value
                       (bytes "\r\n*2\r\n$3\r\nGET\r\n$1\r\na\r\n*2\r\n$3\r\nGET\r\n$1\r\nb\r\n"))
               (read-bytes sent))
            "every command reaches the wire, bytes raw, in order")))
(println "  commands over a built connection: ok")

# ── RESP self-tests (no Redis needed) ───────────────────────────────────────

(println "Running RESP self-tests...")
(redis:test)
(println "RESP self-tests passed.")
