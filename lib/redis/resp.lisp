(elle/epoch 15)
# audited: 2026-10-26
## The RESP2 codec: encode a command, read a reply from a connection, and the connection a reply is read from.
## lib/redis/overview.md
##
## Loaded via: (def resp ((import "std/redis/resp")))
##
## Exports: {:encode :read :read-raw :ok? :bool :connection :connect :close
##           :test}

(def framing ((import "std/framing")))

## ── RESP2 encoder ─────────────────────────────────────────────────────

(defn resp-encode [& args]
  "Encode a Redis command as a RESP2 array of bulk strings.
   Each argument is converted to a string and sent as a bulk string."
  (def buf @"")
  (push buf (string "*" (length args) "\r\n"))
  (each arg in args
    (let [s (string arg)]
      (push buf (string "$" (string/size-of s) "\r\n" s "\r\n"))))
  (freeze buf))

## ── RESP2 decoder ─────────────────────────────────────────────────────

(defn read-reply-line [conn]
  "The next line of conn's reply stream as a string, or nil at the end."
  (let [line (framing:line conn:framer)]
    (when line (string line))))

(defn resp-read [conn]
  "Read a single RESP2 reply from conn. Signals on error replies.
   Returns Elle values: string, integer, array, or nil. A bulk string is
   a view of the framer's chunk."
  (let [line (read-reply-line conn)]
    (when (nil? line)
      (error {:error :redis-error
              :reason :unexpected-eof
              :message "unexpected EOF"}))
    (let [prefix (get line 0)
          body (slice line 1)]
      (case prefix
        # Simple string
        "+" body

        # Error
        "-" (error {:error :redis-error
                    :reason :server-error
                    :body body
                    :message body})

        # Integer
        ":" (parse-int body)

        # Bulk string
        "$"
          (let [len (parse-int body)]
            (if (= len -1)
              nil
              (let [data (framing:exact conn:framer (+ len 2))]
                (when (nil? data)
                  (error {:error :redis-error
                          :reason :unexpected-eof
                          :phase :bulk-string
                          :message "unexpected EOF reading bulk string"}))
                (string (slice data 0 len)))))

        # Array
        "*"
          (let [count (parse-int body)]
            (if (= count -1)
              nil
              (block (def result @[])
                (def @i 0)
                (while (< i count)
                  (push result (resp-read conn))
                  (assign i (+ i 1)))
                (freeze result))))
        (error {:error :redis-error
                :reason :unexpected-prefix
                :prefix prefix
                :message (string "unexpected RESP prefix: " prefix)})))))

(defn resp-read-raw [conn]
  "Read a RESP2 reply, returning errors as structs instead of signaling.
   Used for pipelining where one bad reply shouldn't corrupt state."
  (let [[ok? val] (protect (resp-read conn))]
    (if ok? val val)))

## ── Reply conversions ─────────────────────────────────────────────────

(defn resp-ok? [val]
  "Convert 'OK' string replies to true."
  (if (= val "OK") true val))

(defn resp-bool [val]
  "Convert integer 0/1 replies to boolean."
  (= val 1))

## ── The connection ────────────────────────────────────────────────────

(defn redis-connection [port &named @size]
  "A connection {:port :framer} over a binary port the caller opened.
   :size is the framer's chunk, 65536 unless given. RESP is byte-framed,
   so a text port is refused."
  (default size 65536)
  {:port port :framer (framing:open port :size size)})

(defn redis-connect [host port]
  "Connect to Redis via TCP. Returns a connection {:port :framer}."
  (redis-connection (tcp/connect host port)))

(defn redis-close [conn]
  "Close a Redis connection."
  (port/close conn:port))

## ── Internal self-tests (RESP encoding/decoding, no Redis needed) ─────

(defn run-internal-tests []
  "Self-tests for RESP encoding and decoding."
  # Scratch files live in a fresh directory under the platform temp
  # root (honors TMPDIR); with-temp-dir deletes the whole tree after
  # the body, even when an assert fails, so runs never litter.
  (with-temp-dir dir
                 (defn with-resp-fixture [name payload thunk]
                   "Spit payload into a scratch file under dir, open it as a binary
       read port (RESP framing is byte-exact), and run (thunk conn) over a
       connection on it. The port is closed even when an assert inside
       thunk fails."
                   (let [path (path/join dir name)]
                     (spit path payload)
                     (let [p (port/open-bytes path :read)]
                       (defer
                         (port/close p)
                         (thunk (redis-connection p))))))

                 # resp-encode
                 (assert (= (resp-encode "PING") "*1\r\n$4\r\nPING\r\n")
                         "resp-encode PING")

                 (assert (= (resp-encode "SET" "key" "value")
                            "*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n")
                         "resp-encode SET key value")

                 (assert (= (resp-encode "GET" "mykey")
                            "*2\r\n$3\r\nGET\r\n$5\r\nmykey\r\n")
                         "resp-encode GET mykey")

                 # resp-encode with multibyte string
                 (assert (= (resp-encode "SET" "k" "café")
                            "*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$5\r\ncafé\r\n")
                         "resp-encode multibyte uses byte length")

                 # resp-read: simple string
                 (with-resp-fixture "simple" "+OK\r\n"
                                    (fn [p]
                                      (assert (= (resp-read p) "OK")
                                      "resp-read simple string")))

                 # resp-read: integer
                 (with-resp-fixture "int" ":42\r\n"
                                    (fn [p]
                                      (assert (= (resp-read p) 42)
                                      "resp-read integer")))

                 # resp-read: bulk string
                 (with-resp-fixture "bulk" "$5\r\nhello\r\n"
                                    (fn [p]
                                      (assert (= (resp-read p) "hello")
                                      "resp-read bulk string")))

                 # resp-read: nil bulk string
                 (with-resp-fixture "nil" "$-1\r\n"
                                    (fn [p]
                                      (assert (nil? (resp-read p))
                                      "resp-read nil bulk string")))

                 # resp-read: array
                 (with-resp-fixture "arr" "*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"
                                    (fn [p]
                                      (let [result (resp-read p)]
                                        (assert (= (length result) 2)
                                        "resp-read array length")
                                        (assert (= (get result 0) "foo")
                                        "resp-read array element 0")
                                        (assert (= (get result 1) "bar")
                                        "resp-read array element 1"))))

                 # resp-read: error
                 (with-resp-fixture "err" "-ERR unknown command\r\n"
                                    (fn [p]
                                      (let [[ok? val] (protect (resp-read p))]
                                        (assert (not ok?)
                                        "resp-read error signals")
                                        (assert (= (get val :error) :redis-error)
                                        "resp-read error kind")
                                        (assert (= (get val :message)
                                        "ERR unknown command")
                                        "resp-read error message"))))

                 # resp-read-raw: error returns struct instead of signaling
                 (with-resp-fixture "raw-err" "-ERR bad\r\n"
                                    (fn [p]
                                      (let [result (resp-read-raw p)]
                                        (assert (struct? result)
                                        "resp-read-raw error is struct")
                                        (assert (= (get result :error)
                                        :redis-error) "resp-read-raw error kind"))))

                 # resp-ok?
                 (assert (= (resp-ok? "OK") true) "resp-ok? OK")
                 (assert (= (resp-ok? "QUEUED") "QUEUED") "resp-ok? passthrough")

                 # resp-bool
                 (assert (= (resp-bool 1) true) "resp-bool 1")
                 (assert (= (resp-bool 0) false) "resp-bool 0")

                 # resp-read: nested array
                 (with-resp-fixture "nested"
                                    "*2\r\n*2\r\n:1\r\n:2\r\n*2\r\n:3\r\n:4\r\n"
                                    (fn [p]
                                      (let [result (resp-read p)]
                                        (assert (= (length result) 2)
                                        "nested array length")
                                        (assert (= (get (get result 0) 0) 1)
                                        "nested array [0][0]")
                                        (assert (= (get (get result 1) 1) 4)
                                        "nested array [1][1]"))))

                 # resp-read: empty array
                 (with-resp-fixture "empty-arr" "*0\r\n"
                                    (fn [p]
                                      (let [result (resp-read p)]
                                        (assert (= (length result) 0)
                                        "empty array")))) true))

## ── Exports ───────────────────────────────────────────────────────────

(fn []
  {:encode resp-encode
   :read resp-read
   :read-raw resp-read-raw
   :ok? resp-ok?
   :bool resp-bool
   :connection redis-connection
   :connect redis-connect
   :close redis-close
   :test run-internal-tests})
