(elle/epoch 13)
# audited: 2026-09-28
## The HTTP/1.1 wire format: header lines, request and status lines, bodies, and reason phrases.
## lib/http/overview.md
##
## Loaded via:
##   (def wire ((import "std/http/wire") :transport transport :chunked chunked))
##
## Exports: {:header->kw :kw->header :read-headers :write-headers
##           :read-request-line :write-request-line :read-status-line
##           :write-status-line :read-body :reason-phrases
##           :build-request-headers :test}

(fn [&named transport chunked]
  (def t-read transport:read)
  (def t-read-line transport:read-line)
  (def t-write transport:write)
  (def t-flush transport:flush)

  ## ── Header names ─────────────────────────────────────────────────────

  (defn header->kw [name]
    "Convert HTTP header name string to lowercase keyword.
     'Content-Type' -> :content-type"
    (keyword (string/lowercase name)))

  (defn capitalize-segment [part]
    "Capitalize first letter of a string segment."
    (if (empty? part)
      part
      (concat (string/uppercase (first part)) (rest part))))

  (defn kw->header [kw]
    "Convert keyword to HTTP header name with capitalized segments.
     :content-type -> 'Content-Type'"
    (let* [parts (string/split (string kw) "-")
           capitalized (map capitalize-segment parts)]
      (string/join capitalized "-")))

  ## ── Headers ──────────────────────────────────────────────────────────

  (defn read-headers [t]
    "Read HTTP headers from a transport until blank line. Returns an
     immutable struct with lowercase-keyword keys (:content-type, :host).
     Signals :http-error on malformed header lines."
    (def headers @{})
    (forever
      (let [line (t-read-line t)]
        (when (or (nil? line) (empty? line)) (break (freeze headers)))
        (let [colon-pos (string/find line ":")]
          (when (nil? colon-pos)
            (error {:error :http-error
                    :reason :malformed-header
                    :line line
                    :message "malformed header"}))
          (let* [name (slice line 0 colon-pos)
                 value (string/trim (slice line (inc colon-pos)))]
            (put headers (header->kw name) value))))))

  (defn write-headers [t headers]
    "Write HTTP headers struct to a transport as 'Name: value\\r\\n' lines.
     Keys are keywords converted back to HTTP header-name casing."
    (each [key value] in (pairs headers)
      (t-write t (string/format "{}: {}\r\n" (kw->header key) value))))

  (defn build-request-headers [host extra-headers body keep-alive]
    "Build request headers. Sets Connection: keep-alive or close."
    (let [headers (merge {:host host
                          :connection (if keep-alive "keep-alive" "close")}
                         (freeze (or extra-headers {})))]
      (if (nil? body)
        headers
        (merge headers {:content-length (string (string/size-of body))}))))

  ## ── Request and status lines ─────────────────────────────────────────

  (defn read-request-line [t]
    "Read and parse HTTP request line: 'GET /path HTTP/1.1'.
     Returns {:method :path :version} or nil on EOF."
    (let [line (t-read-line t)]
      (if (nil? line)
        nil
        (let [parts (string/split line " ")]
          (when (< (length parts) 3)
            (error {:error :http-error
                    :reason :malformed-request-line
                    :line line
                    :message "malformed request line"}))
          (let [[method path version] parts]
            (unless (string/starts-with? version "HTTP/")
              (error {:error :http-error
                      :reason :invalid-http-version
                      :version version
                      :message "invalid HTTP version"}))
            {:method method :path path :version version})))))

  (defn write-request-line [t method path]
    "Write HTTP request line: 'METHOD path HTTP/1.1\\r\\n'."
    (t-write t (string/format "{} {} HTTP/1.1\r\n" method path)))

  (defn read-status-line [t]
    "Read and parse HTTP status line: 'HTTP/1.1 200 OK'.
     Returns {:version :status :reason} where :status is an integer."
    (let* [line (t-read-line t)
           parts (string/split line " ")]
      (when (< (length parts) 2)
        (error {:error :http-error
                :reason :malformed-status-line
                :line line
                :message "malformed status line"}))
      (let* [[version status-str & reason-parts] parts
             status (parse-int status-str)
             reason (if (empty? reason-parts) "" (string/join reason-parts " "))]
        {:version version :status status :reason reason})))

  (defn write-status-line [t status reason]
    "Write HTTP status line: 'HTTP/1.1 status reason\\r\\n'."
    (t-write t (string/format "HTTP/1.1 {} {}\r\n" status reason)))

  ## ── Bodies ───────────────────────────────────────────────────────────

  (defn read-fixed-body [t n]
    "Read exactly n bytes from transport and return as a string.
     Loops on short reads."
    (if (= n 0)
      ""
      (begin
        (def @remaining n)
        (def @buf (@bytes))
        (while (pos? remaining)
          (let [chunk (t-read t remaining)]
            (when (nil? chunk)
              (error {:error :http-error
                      :reason :unexpected-eof
                      :phase :body
                      :message "unexpected EOF reading body"}))
            (let [b (if (bytes? chunk) chunk (bytes chunk))]
              (append buf b)
              (assign remaining (- remaining (length b))))))
        (string (freeze buf)))))

  (defn read-body [t headers]
    "Read request/response body from transport.
     If Transfer-Encoding: chunked is set, reads chunks and reassembles
     (taking precedence over Content-Length per RFC 7230 §3.3.3).
     Otherwise falls back to Content-Length. Returns body string, or nil
     if neither framing header is present."
    (cond
      (chunked:chunked? headers) (chunked:read-body t)
      headers:content-length (read-fixed-body t
      (parse-int headers:content-length))
      true nil))

  ## ── Reason phrases ───────────────────────────────────────────────────

  (def reason-phrases
    {200 "OK"
     201 "Created"
     204 "No Content"
     301 "Moved Permanently"
     302 "Found"
     304 "Not Modified"
     400 "Bad Request"
     401 "Unauthorized"
     403 "Forbidden"
     404 "Not Found"
     405 "Method Not Allowed"
     413 "Payload Too Large"
     409 "Conflict"
     500 "Internal Server Error"
     502 "Bad Gateway"
     503 "Service Unavailable"})

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on header, line and body framing."
    # Scratch files live in a fresh directory under the platform temp
    # root (honors TMPDIR); with-temp-dir deletes the whole tree after
    # the body, even when an assert fails, so runs never litter.
    (with-temp-dir dir
                   (defn scratch [name]
                     "Path for a scratch file under this run's temp dir."
                     (path/join dir name))
                   (def with-file-transport transport:with-file)

                   # header->kw
                   (assert (= (header->kw "Content-Type") :content-type)
                           "header->kw Content-Type")
                   (assert (= (header->kw "Host") :host) "header->kw Host")
                   (assert (= (header->kw "X-Custom-Header") :x-custom-header)
                           "header->kw X-Custom-Header")
                   (assert (= (header->kw "content-type") :content-type)
                           "header->kw lowercase")

                   # kw->header
                   (assert (= (kw->header :content-type) "Content-Type")
                           "kw->header content-type")
                   (assert (= (kw->header :host) "Host") "kw->header host")
                   (assert (= (kw->header :x-custom-header) "X-Custom-Header")
                           "kw->header x-custom-header")
                   (assert (= (kw->header :content-length) "Content-Length")
                           "kw->header content-length")

                   # round-trip
                   (assert (= (kw->header (header->kw "Content-Type"))
                              "Content-Type") "header round-trip Content-Type")
                   (assert (= (kw->header (header->kw "Host")) "Host")
                           "header round-trip Host")

                   # read-headers via file transport
                   (spit (scratch "headers")
                         "Content-Type: text/plain\r\nHost: example.com\r\nContent-Length: 42\r\n\r\n")
                   (with-file-transport (scratch "headers")
                                        :read (fn [t]
                                          (let [h (read-headers t)]
                                            (assert (= h:content-type
                                            "text/plain")
                                            "read-headers content-type")
                                            (assert (= h:host "example.com")
                                            "read-headers host")
                                            (assert (= h:content-length "42")
                                            "read-headers content-length"))))

                   # read-headers trims whitespace
                   (spit (scratch "headers-ws") "X-Foo:   bar baz   \r\n\r\n")
                   (with-file-transport (scratch "headers-ws")
                                        :read (fn [t]
                                          (let [h (read-headers t)]
                                            (assert (= h:x-foo "bar baz")
                                            "read-headers trims whitespace"))))

                   # read-headers malformed
                   (spit (scratch "headers-bad") "no-colon-here\r\n\r\n")
                   (with-file-transport (scratch "headers-bad")
                                        :read (fn [t]
                                          (let [[ok? _] (protect (read-headers t))]
                                            (assert (not ok?)
                                            "read-headers malformed signals error"))))

                   # write-headers
                   (with-file-transport (scratch "write-headers")
                                        :write (fn [t]
                                          (write-headers t
                                          {:content-type "text/plain"
                                          :content-length "11"})
                                          (t-write t "\r\n")
                                          (t-flush t)))
                   (let [content (slurp (scratch "write-headers"))]
                     (assert (string/contains? content
                             "Content-Type: text/plain")
                             "write-headers content-type")
                     (assert (string/contains? content "Content-Length: 11")
                             "write-headers content-length"))

                   # read-request-line
                   (spit (scratch "req-line") "GET /path HTTP/1.1\r\n")
                   (with-file-transport (scratch "req-line")
                                        :read (fn [t]
                                          (let [rl (read-request-line t)]
                                            (assert (= rl:method "GET")
                                            "request-line method")
                                            (assert (= rl:path "/path")
                                            "request-line path")
                                            (assert (= rl:version "HTTP/1.1")
                                            "request-line version"))))

                   # read-status-line
                   (spit (scratch "status-200") "HTTP/1.1 200 OK\r\n")
                   (with-file-transport (scratch "status-200")
                                        :read (fn [t]
                                          (let [sl (read-status-line t)]
                                            (assert (= sl:version "HTTP/1.1")
                                            "status-line version")
                                            (assert (= sl:status 200)
                                            "status-line status")
                                            (assert (= sl:reason "OK")
                                            "status-line reason"))))

                   # read-body with Content-Length
                   (spit (scratch "body") "hello world")
                   (with-file-transport (scratch "body")
                                        :read (fn [t]
                                          (let [body (read-body t
                                            {:content-length "11"})]
                                            (assert (= body "hello world")
                                            "read-body with content-length"))))

                   # read-body without Content-Length
                   (spit (scratch "body-no-cl") "ignored")
                   (with-file-transport (scratch "body-no-cl")
                                        :read (fn [t]
                                          (let [body (read-body t {})]
                                            (assert (nil? body)
                                            "read-body without content-length is nil"))))

                   # read-body dispatches on Transfer-Encoding before Content-Length
                   (spit (scratch "body-chunked") "5\r\nhello\r\n0\r\n\r\n")
                   (with-file-transport (scratch "body-chunked")
                                        :read (fn [t]
                                          (let [body (read-body t
                                            {:transfer-encoding "chunked"
                                            :content-length "999"})]
                                            (assert (= body "hello")
                                            "read-body prefers chunked over content-length"))))

                   # full request parse
                   (spit (scratch "full-req")
                         "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
                   (let [out @[nil nil nil nil]]
                     (with-file-transport (scratch "full-req")
                     :read (fn [t]
                             (let [rl (read-request-line t)]
                               (put out 0 rl:method)
                               (put out 1 rl:path)
                               (let [h (read-headers t)]
                                 (put out 2 h:host)
                                 (let [body (read-body t h)]
                                   (put out 3 body))))))
                     (assert (= (get out 0) "POST") "full req method")
                     (assert (= (get out 1) "/submit") "full req path")
                     (assert (= (get out 2) "localhost") "full req host")
                     (assert (= (get out 3) "data") "full req body")) true))

  {:header->kw header->kw
   :kw->header kw->header
   :read-headers read-headers
   :write-headers write-headers
   :read-request-line read-request-line
   :write-request-line write-request-line
   :read-status-line read-status-line
   :write-status-line write-status-line
   :read-body read-body
   :reason-phrases reason-phrases
   :build-request-headers build-request-headers
   :test run-tests})
