(elle/epoch 13)
# audited: 2026-09-28
## HTTP/1.1 chunked transfer encoding: reading a chunked body and writing one.
## lib/http/overview.md
##
## Loaded via:
##   (def chunked ((import "std/http/chunked") :transport transport))
##
## Exports: {:chunk-size :read-body :chunked? :write-chunk :write-last-chunk
##           :test}

(fn [&named transport]
  (def t-read transport:read)
  (def t-read-line transport:read-line)
  (def t-write transport:write)
  (def t-flush transport:flush)

  (defn chunk-size [line]
    "Parse a chunk-size line per RFC 7230: hex digits with optional
     ';ext=...' extensions. Signals :http-error on malformed input."
    (when (nil? line)
      (error {:error :http-error
              :reason :unexpected-eof
              :phase :chunk-size
              :message "unexpected EOF reading chunk size"}))
    (let* [semi (string/find line ";")
           hex (string/trim (if (nil? semi) line (slice line 0 semi)))]
      (when (empty? hex)
        (error {:error :http-error
                :reason :malformed-chunk-size
                :line line
                :message "malformed chunk size"}))
      (parse-int hex 16)))

  (defn read-chunked-body [t]
    "Read an HTTP/1.1 Transfer-Encoding: chunked body from transport.
     Reads chunks until the terminating 0-chunk, discards chunk extensions
     and trailers, returns the reassembled body as a string."
    (def @buf (@bytes))
    (block :chunks
      (forever
        (let [size (chunk-size (t-read-line t))]
          (when (= size 0)
            # Consume optional trailers until the blank line.
            (forever
              (let [line (t-read-line t)]
                (when (or (nil? line) (empty? line)) (break))))
            (break :chunks nil))
          (def @remaining size)
          (while (pos? remaining)
            (let [chunk (t-read t remaining)]
              (when (nil? chunk)
                (error {:error :http-error
                        :reason :unexpected-eof
                        :phase :chunk-data
                        :message "unexpected EOF reading chunk data"}))
              (let [b (if (bytes? chunk) chunk (bytes chunk))]
                (append buf b)
                (assign remaining (- remaining (length b))))))
          # Consume the CRLF that terminates the chunk data.
          (t-read-line t))))
    (string (freeze buf)))

  (defn chunked? [headers]
    "True when headers indicate Transfer-Encoding: chunked."
    (let [te headers:transfer-encoding]
      (and te (string/contains? (string/lowercase te) "chunked"))))

  (defn write-chunk [t data]
    "Write a single chunk to transport in HTTP/1.1 chunked encoding.
     data may be a string or bytes. Writing an empty chunk is a no-op
     (callers must still invoke write-last-chunk to terminate the body)."
    (let* [b (if (bytes? data) data (bytes data))
           n (length b)]
      (when (pos? n)
        (t-write t (string/format "{}\r\n" (number->string n 16)))
        (t-write t b)
        (t-write t "\r\n"))))

  (defn write-last-chunk [t]
    "Write the terminating zero-length chunk and trailer CRLF.
     Every chunked body must end with this."
    (t-write t "0\r\n\r\n"))

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on chunk framing, read and write."
    # Scratch files live in a fresh directory under the platform temp
    # root (honors TMPDIR); with-temp-dir deletes the whole tree after
    # the body, even when an assert fails, so runs never litter.
    (with-temp-dir dir
                   (defn scratch [name]
                     "Path for a scratch file under this run's temp dir."
                     (path/join dir name))
                   (def with-file-transport transport:with-file)

                   # chunk-size: hex digits, with optional extension, error cases
                   (assert (= (chunk-size "0") 0) "chunk-size 0")
                   (assert (= (chunk-size "a") 10) "chunk-size a")
                   (assert (= (chunk-size "1a") 26) "chunk-size 1a")
                   (assert (= (chunk-size "FF") 255) "chunk-size FF (uppercase)")
                   (assert (= (chunk-size "10;ext=value") 16)
                           "chunk-size with extension")
                   (assert (= (chunk-size "  20  ") 32)
                           "chunk-size trims whitespace")
                   (let [[ok? _] (protect (chunk-size nil))]
                     (assert (not ok?) "chunk-size nil signals error"))
                   (let [[ok? _] (protect (chunk-size ""))]
                     (assert (not ok?) "chunk-size empty signals error"))
                   (let [[ok? _] (protect (chunk-size ";ext"))]
                     (assert (not ok?) "chunk-size bare extension signals error"))

                   # chunked? predicate
                   (assert (chunked? {:transfer-encoding "chunked"})
                           "chunked? lowercase")
                   (assert (chunked? {:transfer-encoding "Chunked"})
                           "chunked? mixed-case")
                   (assert (chunked? {:transfer-encoding "gzip, chunked"})
                           "chunked? with gzip")
                   (assert (not (chunked? {})) "chunked? absent")
                   (assert (not (chunked? {:transfer-encoding "gzip"}))
                           "chunked? gzip-only")

                   # read-chunked-body: happy path
                   (spit (scratch "chunked")
                         "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")
                   (with-file-transport (scratch "chunked")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body "hello world")
                                            "read-chunked-body concatenates chunks"))))

                   # read-chunked-body: chunk extensions are ignored
                   (spit (scratch "chunked-ext") "3;ext=foo\r\nabc\r\n0\r\n\r\n")
                   (with-file-transport (scratch "chunked-ext")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body "abc")
                                            "read-chunked-body ignores extensions"))))

                   # read-chunked-body: trailers are discarded
                   (spit (scratch "chunked-trail")
                         "4\r\ndata\r\n0\r\nX-Trailer: value\r\n\r\n")
                   (with-file-transport (scratch "chunked-trail")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body "data")
                                            "read-chunked-body discards trailers"))))

                   # read-chunked-body: hex sizes
                   (spit (scratch "chunked-hex")
                         "1a\r\nabcdefghijklmnopqrstuvwxyz\r\n0\r\n\r\n")
                   (with-file-transport (scratch "chunked-hex")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body
                                            "abcdefghijklmnopqrstuvwxyz")
                                            "read-chunked-body hex-encoded size"))))

                   # read-chunked-body: empty body (just the 0-chunk)
                   (spit (scratch "chunked-empty") "0\r\n\r\n")
                   (with-file-transport (scratch "chunked-empty")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body "")
                                            "read-chunked-body empty body"))))

                   # write-chunk: produces hex-size + CRLF + data + CRLF
                   (with-file-transport (scratch "write-chunk")
                                        :write (fn [t]
                                          (write-chunk t "hi")
                                          (write-chunk t "there")
                                          (write-last-chunk t)
                                          (t-flush t)))
                   (let [content (slurp (scratch "write-chunk"))]
                     (assert (= content "2\r\nhi\r\n5\r\nthere\r\n0\r\n\r\n")
                             "write-chunk + write-last-chunk produce correct framing"))

                   # write-chunk: empty chunk is a no-op
                   (with-file-transport (scratch "write-chunk-empty")
                                        :write (fn [t]
                                          (write-chunk t "")
                                          (write-last-chunk t)
                                          (t-flush t)))
                   (let [content (slurp (scratch "write-chunk-empty"))]
                     (assert (= content "0\r\n\r\n")
                             "write-chunk empty is a no-op"))

                   # round-trip: write chunks, read them back
                   (with-file-transport (scratch "chunk-roundtrip")
                                        :write (fn [t]
                                          (write-chunk t "one ")
                                          (write-chunk t "two ")
                                          (write-chunk t "three")
                                          (write-last-chunk t)
                                          (t-flush t)))
                   (with-file-transport (scratch "chunk-roundtrip")
                                        :read (fn [t]
                                          (let [body (read-chunked-body t)]
                                            (assert (= body "one two three")
                                            "chunk round-trip")))) true))

  {:chunk-size chunk-size
   :read-body read-chunked-body
   :chunked? chunked?
   :write-chunk write-chunk
   :write-last-chunk write-last-chunk
   :test run-tests})
