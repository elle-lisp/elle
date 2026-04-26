(elle/epoch 9)
## lib/http.lisp — Pure Elle HTTP/1.1 client and server
##
## Plain HTTP only:
##   (def http ((import "std/http")))
##
## HTTPS client support requires a TLS plugin passed as :tls:
##   (def tls-plug (import "plugin/tls"))
##   (def http ((import "std/http") :tls tls-plug))
##
## Usage: (http:get "http://example.com/")   (http:get "https://...")
##
## Future plugins (DNS overrides, proxies, etc.) can be added as more
## &named args on the constructor without breaking existing callers.

(fn [&named tls compress]

  ## ── Optional compress plugin ─────────────────────────────────────────
  ## Accept either a pre-imported compress struct (from (import "std/compress"))
  ## or literal `true` to have this module import it for us. Exposes the
  ## standard compress helpers (gzip/gunzip/zlib/unzlib/deflate/inflate/
  ## zstd/unzstd) as http:<name>. No automatic Accept-Encoding negotiation —
  ## callers apply these explicitly to bodies or chunks.

  (def compress-mod
    (cond
      (nil? compress)       nil
      (= compress true)     ((import "std/compress"))
      (struct? compress)    compress
      true (error {:error :http-error :reason :bad-compress :value compress
                    :message ":compress must be nil, true, or a compress module struct"})))

  (defn require-compress []
    "Return the configured compress module, or signal a clear error if
     :compress was not supplied at module init."
    (when (nil? compress-mod)
      (error {:error :http-error :reason :compress-not-configured
              :message ":compress option was not supplied to (import \"std/http\")"}))
    compress-mod)

  (defn compress-gzip    [data & opts] (let [c (require-compress)] (apply c:gzip    data opts)))
  (defn compress-gunzip  [data]        (let [c (require-compress)] (c:gunzip  data)))
  (defn compress-zlib    [data & opts] (let [c (require-compress)] (apply c:zlib    data opts)))
  (defn compress-unzlib  [data]        (let [c (require-compress)] (c:unzlib  data)))
  (defn compress-deflate [data & opts] (let [c (require-compress)] (apply c:deflate data opts)))
  (defn compress-inflate [data]        (let [c (require-compress)] (c:inflate data)))
  (defn compress-zstd    [data & opts] (let [c (require-compress)] (apply c:zstd    data opts)))
  (defn compress-unzstd  [data]        (let [c (require-compress)] (c:unzstd  data)))


  ## ── URL parsing ──────────────────────────────────────────────────────

  (def url-schemes
    {"http"  {:prefix-len 7 :default-port 80}
     "https" {:prefix-len 8 :default-port 443}})

  (defn pick-scheme [url]
    "Return [scheme info] for url, or nil if no supported prefix matches."
    (cond
      (string/starts-with? url "https://") ["https" (get url-schemes "https")]
      (string/starts-with? url "http://")  ["http"  (get url-schemes "http")]
      true nil))

  (defn parse-url [url]
    "Parse an HTTP or HTTPS URL string into {:scheme :host :port :path :query}.
     Supports 'http' (default port 80) and 'https' (default port 443).
     Default path is '/'. Query is nil if absent, otherwise the string after '?'
     without the '?'."
    (let [picked (pick-scheme url)]
      (when (nil? picked)
        (error {:error :http-error :reason :unsupported-scheme :url url
                :message "unsupported scheme"}))
      (let* [[scheme info] picked
             tail (slice url info:prefix-len)
             slash (string/find tail "/")
             auth (if (nil? slash) tail (slice tail 0 slash))
             path+query (if (nil? slash) "/" (slice tail slash))
             colon (string/find auth ":")
             host (if (nil? colon) auth (slice auth 0 colon))
             port (if (nil? colon) info:default-port (parse-int (slice auth (inc colon))))]
        (when (empty? tail)
          (error {:error :http-error :reason :missing-host :url url :message "missing host"}))
        (when (empty? host)
          (error {:error :http-error :reason :empty-host :url url :message "empty host"}))
        (let* [q-pos (string/find path+query "?")
               path (if (nil? q-pos) path+query (slice path+query 0 q-pos))
               query (if (nil? q-pos) nil (slice path+query (inc q-pos)))]
          {:scheme scheme :host host :port port :path path :query query}))))

  ## ── Query-string encoding ────────────────────────────────────────────

  (defn query-scalar->string [v]
    "Render a scalar value into its query-string form. Booleans go to
     'true'/'false'; everything else goes through (string v)."
    (cond
      (boolean? v) (if v "true" "false")
      true         (string v)))

  (defn query-encode-pair [ekey v parts]
    "Push 'key=value' into parts (uri-encoded). Skip nil, recurse into
     arrays/lists to produce repeated 'key=v1&key=v2' pairs."
    (cond
      (nil? v) nil
      (or (array? v) (list? v))
       (each elt in v
         (unless (nil? elt)
           (push parts
             (string/format "{}={}" ekey (uri-encode (query-scalar->string elt))))))
      true
       (push parts
         (string/format "{}={}" ekey (uri-encode (query-scalar->string v))))))

  (defn query-encode [params]
    "Encode a struct/map of query parameters as an
     application/x-www-form-urlencoded string: 'k1=v1&k2=v2'. Keys and
     values are percent-encoded per RFC 3986. Array/list values produce
     repeated 'key=v1&key=v2' pairs. nil values are omitted."
    (let [parts @[]]
      (each [k v] in (pairs params)
        (let [ekey (uri-encode (string k))]
          (query-encode-pair ekey v parts)))
      (string/join (freeze parts) "&")))

  (defn merge-query [url-query extra]
    "Combine a URL's existing query string with caller-supplied extra.
     extra is nil, a string (used as-is), or a struct (query-encoded).
     Returns the merged query string, or nil if both are absent."
    (let [encoded (cond
                     (nil? extra)    nil
                     (string? extra) extra
                     true            (query-encode extra))]
      (cond
        (and url-query encoded) (string url-query "&" encoded)
        true                    (or url-query encoded))))

  ## ── Header parsing and serialization ─────────────────────────────────

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

  ## ── Transport abstraction ────────────────────────────────────────────
  ##
  ## A transport is a struct of closures exposing the subset of port-like
  ## operations HTTP needs: {:read :read-line :write :flush :close}. All
  ## wire-format helpers take a transport, so the same code path handles
  ## plain TCP ports and TLS connections.

  (defn tcp-transport [port]
    "Wrap a plain TCP (or file) port as a transport.
     Writes are buffered in user space; flush sends a single port/write
     to avoid per-line scheduler yields on the io_uring path."
    (def @wbuf-parts @[])
    {:read      (fn [n] (port/read port n))
     :read-line (fn [] (port/read-line port))
     :write     (fn [data]
                  (let [d (if (bytes? data) data (bytes data))]
                    (push wbuf-parts d)))
     :flush     (fn []
                  (when (> (length wbuf-parts) 0)
                    (let [combined (apply concat (freeze wbuf-parts))]
                      (port/write port combined)
                      (assign wbuf-parts @[]))))
     :close     (fn [] (port/close port))})

  (defn strip-line-terminator [s]
    "Strip a trailing CRLF, LF, or CR from a line. CRLF is a single
     grapheme in Elle, so all three cases drop one grapheme."
    (if (and s (or (string/ends-with? s "\r\n")
                   (string/ends-with? s "\n")
                   (string/ends-with? s "\r")))
        (slice s 0 (dec (length s)))
        s))

  (defn tls-transport [conn]
    "Wrap a TLS connection (from the configured tls plugin) as a transport.
     Only available when :tls was passed to the module initializer.

     Note: port/read-line strips trailing newlines; tls:read-line does
     not. We normalize here so the wire-format helpers see the same
     semantics regardless of transport."
    {:read      (fn [n] (tls:read conn n))
     :read-line (fn [] (let [line (tls:read-line conn)]
                         (when line (strip-line-terminator line))))
     :write     (fn [data] (tls:write conn data))
     :flush     (fn [] nil)
     :close     (fn [] (tls:close conn))})

  (defn open-transport [url-parsed]
    "Open a transport to the URL's host:port. Uses TLS when the scheme is
     https and a tls plugin was supplied to the module initializer.
     Signals :http-error :tls-not-configured if an https URL is used
     without a tls plugin."
    (cond
      (= url-parsed:scheme "https") (begin (when (nil? tls)
         (error {:error :http-error :reason :tls-not-configured
                 :url url-parsed
                 :message "https URL requires the tls plugin; pass :tls to (import \"std/http\")"})) (tls-transport (tls:connect url-parsed:host url-parsed:port)))
      true
       (tcp-transport (tcp/connect url-parsed:host url-parsed:port))))

  (defn t-read       [t n]    ((get t :read) n))
  (defn t-read-line  [t]      ((get t :read-line)))
  (defn t-write      [t data] ((get t :write) data))
  (defn t-flush      [t]      ((get t :flush)))
  (defn t-close      [t]      ((get t :close)))

  ## ── Wire format: headers ─────────────────────────────────────────────

  (defn read-headers [t]
    "Read HTTP headers from a transport until blank line. Returns an
     immutable struct with lowercase-keyword keys (:content-type, :host).
     Signals :http-error on malformed header lines."
    (def headers @{})
    (forever
      (let [line (t-read-line t)]
        (when (or (nil? line) (empty? line))
          (break (freeze headers)))
        (let [colon-pos (string/find line ":")]
          (when (nil? colon-pos)
            (error {:error :http-error :reason :malformed-header :line line
                    :message "malformed header"}))
          (let* [name (slice line 0 colon-pos)
                 value (string/trim (slice line (inc colon-pos)))]
            (put headers (header->kw name) value))))))

  (defn write-headers [t headers]
    "Write HTTP headers struct to a transport as 'Name: value\\r\\n' lines.
     Keys are keywords converted back to HTTP header-name casing."
    (each [key value] in (pairs headers)
      (t-write t (string/format "{}: {}\r\n" (kw->header key) value))))

  ## ── Wire format: request / status lines ──────────────────────────────

  (defn read-request-line [t]
    "Read and parse HTTP request line: 'GET /path HTTP/1.1'.
     Returns {:method :path :version} or nil on EOF."
    (let [line (t-read-line t)]
      (if (nil? line)
        nil
        (let [parts (string/split line " ")]
          (when (< (length parts) 3)
            (error {:error :http-error :reason :malformed-request-line :line line
                    :message "malformed request line"}))
          (let [[method path version] parts]
            (unless (string/starts-with? version "HTTP/")
              (error {:error :http-error :reason :invalid-http-version :version version
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
        (error {:error :http-error :reason :malformed-status-line :line line
                :message "malformed status line"}))
      (let* [[version status-str & reason-parts] parts
             status (parse-int status-str)
             reason (if (empty? reason-parts) "" (string/join reason-parts " "))]
        {:version version :status status :reason reason})))

  (defn write-status-line [t status reason]
    "Write HTTP status line: 'HTTP/1.1 status reason\\r\\n'."
    (t-write t (string/format "HTTP/1.1 {} {}\r\n" status reason)))

  ## ── Wire format: body (fixed-length + chunked) ───────────────────────

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
              (error {:error :http-error :reason :unexpected-eof :phase :body
                      :message "unexpected EOF reading body"}))
            (let [b (if (bytes? chunk) chunk (bytes chunk))]
              (append buf b)
              (assign remaining (- remaining (length b))))))
        (string (freeze buf)))))

  (defn chunk-size [line]
    "Parse a chunk-size line per RFC 7230: hex digits with optional
     ';ext=...' extensions. Signals :http-error on malformed input."
    (when (nil? line)
      (error {:error :http-error :reason :unexpected-eof :phase :chunk-size
              :message "unexpected EOF reading chunk size"}))
    (let* [semi (string/find line ";")
           hex (string/trim (if (nil? semi) line (slice line 0 semi)))]
      (when (empty? hex)
        (error {:error :http-error :reason :malformed-chunk-size :line line
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
                (error {:error :http-error :reason :unexpected-eof :phase :chunk-data
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

  (defn read-body [t headers]
    "Read request/response body from transport.
     If Transfer-Encoding: chunked is set, reads chunks and reassembles
     (taking precedence over Content-Length per RFC 7230 §3.3.3).
     Otherwise falls back to Content-Length. Returns body string, or nil
     if neither framing header is present."
    (cond
      (chunked? headers) (read-chunked-body t)
      headers:content-length (read-fixed-body t (parse-int headers:content-length))
      true nil))

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

  ## ── Response construction ────────────────────────────────────────────

  (defn http-respond [status body &named headers]
    "Build a response struct with Content-Type and Content-Length set.
     Caller can override headers via :headers."
    (let* [base-headers {:content-type "text/plain"
                          :content-length (string (string/size-of body))}
           merged (if (nil? headers)
                     base-headers
                     (merge base-headers (freeze headers)))]
      {:status status :headers merged :body body}))

  ## ── Redirect handling ────────────────────────────────────────────────

  (def redirect-statuses |301 302 303 307 308|)
  (def get-rewrite-statuses |301 302 303|)

  (defn default-redirect-limit [] 10)

  (defn resolve-location [base location]
    "Resolve a redirect Location header value against the base URL
     struct. Handles absolute URLs, scheme-relative ('//host/path'),
     and absolute paths ('/foo'). Other forms are treated as absolute
     paths rooted at '/'."
    (cond
      (or (string/starts-with? location "http://")
           (string/starts-with? location "https://"))
       location
      (string/starts-with? location "//")
       (string base:scheme ":" location)
      true
       (let [path (if (string/starts-with? location "/")
                       location
                       (string "/" location))]
         (string base:scheme "://" base:host ":" base:port path))))

  (defn redirect-limit [follow]
    "Normalize the :follow-redirects option: nil/false → 0, true →
     default, integer → itself."
    (cond
      (nil? follow)           0
      (= follow false)        0
      (= follow true)         (default-redirect-limit)
      (integer? follow)       follow
      true (error {:error :http-error :reason :bad-follow-redirects
                    :value follow
                    :message ":follow-redirects must be nil, true, or a non-negative integer"})))

  ## ── Client API ───────────────────────────────────────────────────────

  (defn wants-close? [headers]
    "True if headers indicate the connection should close."
    (let [conn headers:connection]
      (and conn (= (string/lowercase conn) "close"))))

  (defn build-request-headers [host extra-headers body keep-alive]
    "Build request headers. Sets Connection: keep-alive or close."
    (let [headers (merge {:host host
                           :connection (if keep-alive "keep-alive" "close")}
                          (freeze (or extra-headers {})))]
      (if (nil? body)
          headers
          (merge headers {:content-length (string (string/size-of body))}))))

  (defn send-request [t method path host extra-headers body keep-alive]
    "Send an HTTP request on an open transport. Returns response struct.
     Does NOT close the transport."
    (write-request-line t method path)
    (let [headers (build-request-headers host extra-headers body keep-alive)]
      (write-headers t headers)
      (t-write t "\r\n")
      (unless (nil? body) (t-write t body))
      (t-flush t)
      (let* [status-line (read-status-line t)
             resp-headers (read-headers t)
             resp-body (read-body t resp-headers)]
        {:status status-line:status :headers resp-headers :body resp-body})))

  (defn do-request [method url-parsed headers body query]
    "Issue a single HTTP request against a parsed URL, applying any
     :query struct/string to the request path, and return the response
     struct. Opens and closes a fresh transport."
    (let* [full-query (merge-query url-parsed:query query)
           request-path (if (nil? full-query)
                             url-parsed:path
                             (string/format "{}?{}" url-parsed:path full-query))
           t (open-transport url-parsed)]
      (defer (protect (t-close t))
        (send-request t method request-path
                      url-parsed:host headers body false))))

  (defn http-request [method url &named body headers query follow-redirects]
    "Make an HTTP/1.1 request. Opens a new transport, sends request, closes.
     Returns {:status :headers :body}. Uses TLS for https URLs if :tls was
     supplied to the module initializer.

     :query is an optional struct or pre-encoded string appended to the
     URL's existing query. Values may be strings, numbers, booleans,
     keywords, or arrays/lists (repeated 'key=v1&key=v2'). nil entries
     are omitted.

     :follow-redirects controls automatic handling of 301/302/303/307/308:
       nil / false (default) — return the redirect response untouched.
       true                  — follow up to 10 hops.
       <integer>             — follow up to N hops.
     Per RFC 9110, 301/302/303 are followed with GET and an empty body;
     307/308 preserve the original method and body. The Location header
     may be absolute, scheme-relative, or an absolute path."
    (def @current-method method)
    (def @current-body body)
    (def @current-query query)
    (def @current-parsed (parse-url url))
    (def @remaining (redirect-limit follow-redirects))
    (def @last-response nil)
    (block :redirects
      (forever
        (let [resp (do-request current-method current-parsed headers
                                current-body current-query)]
          (assign last-response resp)
          (when (or (zero? remaining)
                    (not (redirect-statuses resp:status)))
            (break :redirects nil))
          (let [loc (get resp:headers :location)]
            (when (nil? loc)
              (break :redirects nil))
            (let [next-url (resolve-location current-parsed loc)]
              (assign current-parsed (parse-url next-url)))
            # Drop :query on redirect — Location already carries the
            # redirected query; caller's :query was for the *initial*
            # request only.
            (assign current-query nil)
            (when (get-rewrite-statuses resp:status)
              (assign current-method "GET")
              (assign current-body nil))
            (assign remaining (dec remaining))))))
    last-response)

  (defn http-get [url &named headers query follow-redirects]
    "Make a GET request. Returns {:status :headers :body}."
    (http-request "GET" url :headers headers :query query
                  :follow-redirects follow-redirects))

  (defn http-post [url body &named headers query follow-redirects]
    "Make a POST request with body. Returns {:status :headers :body}."
    (http-request "POST" url :body body :headers headers :query query
                  :follow-redirects follow-redirects))

  (defn http-connect [url]
    "Open a keep-alive transport to a URL's host:port.
     Returns {:transport :host} for use with http:send."
    (let* [url-parsed (parse-url url)
           t (open-transport url-parsed)]
      {:transport t :host url-parsed:host}))

  (defn http-send [session method path &named body headers]
    "Send a request on an existing keep-alive session.
     session: struct from http:connect. Returns {:status :headers :body}.
     Transport remains open unless server sends Connection: close."
    (send-request session:transport method path
                  session:host headers body true))

  (defn http-close [session]
    "Close a keep-alive session."
    (t-close session:transport))

  ## ── Server-Sent Events (SSE) ─────────────────────────────────────────

  (def sse-default-retry-ms 3000)

  (defn sse-strip-leading-space [s]
    "Per the SSE spec, a single leading space on a field value is
     eaten (so 'data: hello' yields data='hello')."
    (if (string/starts-with? s " ") (slice s 1) s))

  (defn sse-parse-field [line]
    "Parse one SSE field line per the HTML spec. Returns {:field :value},
     or nil for comments and unparseable input."
    (cond
      (empty? line) nil
      (string/starts-with? line ":") nil  # comment
      true
       (let [colon (string/find line ":")]
         (cond
           (nil? colon)
            {:field line :value ""}
           true
            {:field (slice line 0 colon)
             :value (sse-strip-leading-space (slice line (inc colon)))}))))

  (defn sse-for-each-body-line [t headers on-line]
    "Call (on-line line) for every body line. Handles both
     Transfer-Encoding: chunked and plain bodies. on-line receives lines
     with trailing CRLF/LF already stripped."
    (if (chunked? headers)
      (sse-for-each-body-line-chunked t on-line)
      (sse-for-each-body-line-plain t on-line)))

  (defn sse-for-each-body-line-plain [t on-line]
    (forever
      (let [line (t-read-line t)]
        (when (nil? line) (break))
        (on-line line))))

  (defn sse-drain-buffered-lines [buf on-line]
    "Yield all complete lines already present in buf. Returns the new
     buf value with the last (incomplete) line left behind."
    (def @remaining buf)
    (block :drain
      (forever
        (let [nl (string/find remaining "\n")]
          (when (nil? nl) (break :drain remaining))
          (let* [raw (slice remaining 0 nl)
                 line (if (string/ends-with? raw "\r")
                           (slice raw 0 (dec (length raw)))
                           raw)]
            (on-line line)
            (assign remaining (slice remaining (inc nl))))))))

  (defn sse-for-each-body-line-chunked [t on-line]
    (def @buf "")
    (block :chunks
      (forever
        (let [size (chunk-size (t-read-line t))]
          (when (= size 0)
            (forever
              (let [line (t-read-line t)]
                (when (or (nil? line) (empty? line)) (break))))
            (unless (empty? buf) (on-line buf))
            (break :chunks))
          (def @remaining size)
          (while (pos? remaining)
            (let [data (t-read t remaining)]
              (when (nil? data)
                (error {:error :http-error :reason :unexpected-eof :phase :chunk-data
                        :message "unexpected EOF reading chunk data"}))
              (let* [b (if (bytes? data) data (bytes data))
                     s (string b)]
                (assign buf (string buf s))
                (assign remaining (- remaining (length b))))))
          (t-read-line t)  # consume trailing CRLF after chunk data
          (assign buf (sse-drain-buffered-lines buf on-line))))))

  (defn sse-dispatch-event [state on-event]
    "If state has accumulated :data, emit an event via on-event and
     reset the per-event accumulators. :id and :retry persist across
     events per the SSE spec."
    (when (pos? (length state:data-lines))
      (on-event {:event (or state:event-type "message")
                 :data  (string/join (freeze state:data-lines) "\n")
                 :id    state:last-id
                 :retry state:retry}))
    (put state :event-type nil)
    (put state :data-lines @[]))

  (defn sse-handle-line [state line on-event]
    "Apply one line of an SSE stream to state. Empty line flushes the
     event; comments and unknown fields are ignored."
    (if (empty? line)
      (sse-dispatch-event state on-event)
      (let [parsed (sse-parse-field line)]
        (when parsed
          (case parsed:field
            "event" (put state :event-type parsed:value)
            "data"  (push state:data-lines parsed:value)
            "id"    (unless (string/contains? parsed:value "\0")
                      (put state :last-id parsed:value))
            "retry" (let [[ok? n] (protect (parse-int parsed:value))]
                      (when (and ok? n (pos? n))
                        (put state :retry n))))))))

  (defn sse-for-each-event [t headers on-event]
    "Parse the SSE body on transport t, calling on-event for each
     complete event. Returns when the stream closes."
    (let [state @{:event-type nil
                   :data-lines @[]
                   :last-id    nil
                   :retry      nil}]
      (sse-for-each-body-line t headers
        (fn [line] (sse-handle-line state line on-event)))))

  (defn sse-open [url headers last-event-id]
    "Open an SSE request. Returns {:transport :status :headers}.
     Body is NOT consumed — caller parses it via sse-for-each-event."
    (let* [url-parsed (parse-url url)
           base-headers {:accept        "text/event-stream"
                          :cache-control "no-cache"}
           with-id (if last-event-id
                      (merge base-headers {:last-event-id last-event-id})
                      base-headers)
           user-headers (freeze (or headers {}))
           final-headers (merge with-id user-headers)
           t (open-transport url-parsed)]
      (write-request-line t "GET" url-parsed:path)
      (write-headers t (build-request-headers
                         url-parsed:host final-headers nil false))
      (t-write t "\r\n")
      (t-flush t)
      (let* [status-line (read-status-line t)
             resp-headers (read-headers t)]
        {:transport t
         :status    status-line:status
         :headers   resp-headers})))

  (defn sse-get [url &named headers last-event-id @reconnect]
    "Open an SSE connection to url and return a coroutine that yields
     events until the stream terminates. Each event is a struct:
       {:event \"message\" :data \"...\" :id \"...\" :retry 3000}

     :reconnect — true (default) or nil/false.
       When true, follows EventSource semantics: on disconnect or
       failure, waits retry-ms (last server-sent :retry, default 3000)
       and reopens with Last-Event-ID. Stops on HTTP 204 No Content.
     :last-event-id — initial Last-Event-ID header.
     :headers — extra request headers merged into the SSE defaults."
    (default reconnect true)
    (coro/new
      (fn []
        (def @current-id last-event-id)
        (def @retry-ms sse-default-retry-ms)
        (block :session
          (forever
            (let* [[ok? result]
                    (protect
                      (let [conn (sse-open url headers current-id)]
                        (defer (protect (t-close conn:transport))
                          (cond
                            (= conn:status 204) :done
                            (and (>= conn:status 200) (< conn:status 300)) (begin (sse-for-each-event conn:transport conn:headers
                               (fn [evt]
                                 (when evt:id    (assign current-id evt:id))
                                 (when evt:retry (assign retry-ms evt:retry))
                                 (yield evt))) :eof)
                            true
                             (error {:error :http-error :reason :sse-bad-status
                                     :status conn:status
                                     :message "SSE: non-2xx response"})))))]
              (cond
                (and ok? (= result :done)) (break :session)
                (not reconnect)            (break :session))
              (ev/sleep (/ retry-ms 1000.0))))))))

  (defn sse-post [url body &named headers]
    "POST to url with body, expecting a text/event-stream response.
     Returns a coroutine that yields events until the server closes.
     Unlike sse-get this does NOT auto-reconnect — POST is typically
     non-idempotent (think LLM streaming: you don't want to silently
     re-submit a prompt).

     Use case: OpenAI-compatible /v1/chat/completions with
     {\"stream\": true} — the body is an SSE stream of token deltas
     terminated by a `data: [DONE]` sentinel the caller can recognize."
    (coro/new
      (fn []
        (let* [url-parsed   (parse-url url)
               base-headers {:accept       "text/event-stream"
                              :cache-control "no-cache"
                              :content-type "application/json"}
               user-headers (freeze (or headers {}))
               final-headers (merge base-headers user-headers)
               t (open-transport url-parsed)]
          (defer (protect (t-close t))
            (write-request-line t "POST" url-parsed:path)
            (write-headers t (build-request-headers
                               url-parsed:host final-headers body false))
            (t-write t "\r\n")
            (unless (nil? body) (t-write t body))
            (t-flush t)
            (let* [status-line  (read-status-line t)
                   resp-headers (read-headers t)]
              (cond
                (and (>= status-line:status 200) (< status-line:status 300))
                 (sse-for-each-event t resp-headers
                   (fn [evt] (yield evt)))
                true
                 (error {:error :http-error :reason :sse-bad-status
                         :status status-line:status
                         :body   (read-body t resp-headers)
                         :message "SSE POST: non-2xx response"}))))))))

  (defn sse-format-field [field value]
    "Serialize one field of an SSE event. Data values with embedded
     newlines are emitted as repeated 'field: line' entries per spec."
    (let [s (string value)]
      (if (and (= field "data") (string/contains? s "\n"))
        (string/join (map (fn [line] (string/format "data: {}\n" line))
                          (string/split s "\n"))
                     "")
        (string/format "{}: {}\n" field s))))

  (defn format-sse-event [evt]
    "Serialize an event struct to SSE wire format. Recognizes :event,
     :data, :id, :retry; unknown fields are omitted. Returns the
     complete frame, terminator included."
    (let [parts @[]]
      (when (and evt:event (not (= evt:event "message")))
        (push parts (sse-format-field "event" evt:event)))
      (when evt:id
        (push parts (sse-format-field "id" evt:id)))
      (when evt:retry
        (push parts (sse-format-field "retry" evt:retry)))
      (when evt:data
        (push parts (sse-format-field "data" evt:data)))
      (string (string/join (freeze parts) "") "\n")))

  (defn sse-response [body-fn &named headers]
    "Build a streaming SSE response. body-fn is a closure (fn [send-event])
     where send-event takes an event struct and emits it to the client.
     Returns a response that uses chunked transfer; suitable for use
     with http:serve."
    (let* [base-headers {:content-type      "text/event-stream"
                          :cache-control     "no-cache"
                          :connection        "keep-alive"
                          :transfer-encoding "chunked"}
           merged (if (nil? headers)
                     base-headers
                     (merge base-headers (freeze headers)))]
      {:status 200
       :headers merged
       :body (fn [write-chunk]
               (body-fn (fn [evt] (write-chunk (format-sse-event evt)))))}))

  ## ── Server API ───────────────────────────────────────────────────────

  (defn read-request [t]
    "Read a complete HTTP request from a transport.
     Returns {:method :path :version :headers :body}, or nil on EOF."
    (when-let [req-line (read-request-line t)]
      (let* [headers (read-headers t)
             body (read-body t headers)]
        {:method req-line:method
         :path req-line:path
         :version req-line:version
         :headers headers
         :body body})))

  (defn write-response [t response]
    "Write a complete HTTP response to a transport and flush.
     response is {:status :headers :body}.
     If the headers declare Transfer-Encoding: chunked, the body is framed
     as chunks. A body that is a function (fn [write-chunk]) is invoked
     with a chunk writer so handlers can stream arbitrary data; otherwise
     the body is written as a single chunk."
    (write-status-line t response:status
                       (or (get reason-phrases response:status) "Unknown"))
    (write-headers t response:headers)
    (t-write t "\r\n")
    (cond
      (chunked? response:headers)
       (let [body response:body]
         (cond
           (nil? body)  nil
           (fn? body)   (body (fn [data] (write-chunk t data)))
           true         (write-chunk t body))
         (write-last-chunk t))
      (not (nil? response:body))
       (t-write t response:body))
    (t-flush t))

  (defn connection-loop [t handler on-error]
    "Handle HTTP requests on a transport until it closes or either side
     sends Connection: close. Errors in handler produce a 500 response.
     on-error is called with (request error) when the handler fails."
    (defer (protect (t-close t))
      (forever
        (let [[ok? req] (protect (read-request t))]
          (unless ok? (break))
          (when (nil? req) (break))
          (let* [[ok? val] (protect (handler req))
                 response (if ok?
                             val
                             (begin
                               (when on-error (on-error req val))
                               (http-respond 500 "Internal Server Error")))]
            (write-response t response)
            (when (or (wants-close? req:headers)
                      (wants-close? response:headers))
              (break)))))))

  (defn default-on-error [request err]
    "Default error handler: print to stderr."
    (port/write (port/stderr)
      (string/format "http: handler error on {} {}: {}\n"
                     request:method request:path err)))

  (defn http-serve [listener handler &named @on-error]
    "Accept connections on listener and handle them with keep-alive.
     Each connection runs in its own fiber via ev/spawn.
     Exits cleanly when the listener is closed.
     handler: (fn [request]) -> response
     :on-error: (fn [request error]) -> nil (default: print to stderr)
     Serves plain HTTP; for HTTPS, the caller can wrap accepted TCP
     connections with tls:accept and pass the resulting tls-conn into
     their own transport (future: first-class https-serve)."
    (default on-error default-on-error)
    (forever
      (let [[ok? conn] (protect (tcp/accept listener))]
        (unless ok? (break))
        (ev/spawn
          (fn [] (connection-loop (tcp-transport conn) handler on-error))))))

  ## ── Internal tests ──────────────────────────────────────────────────

  (defn with-file-transport [path mode thunk]
    "Helper: open file at path as a transport, run thunk, close."
    (let [p (port/open path mode)]
      (defer (protect (port/close p))
        (thunk (tcp-transport p)))))

  (defn run-internal-tests []
    "Sanity checks on internal wire-format helpers. Called via (http:test)."

    # header->kw
    (assert (= (header->kw "Content-Type") :content-type) "header->kw Content-Type")
    (assert (= (header->kw "Host") :host)                 "header->kw Host")
    (assert (= (header->kw "X-Custom-Header") :x-custom-header)
      "header->kw X-Custom-Header")
    (assert (= (header->kw "content-type") :content-type) "header->kw lowercase")

    # kw->header
    (assert (= (kw->header :content-type) "Content-Type")     "kw->header content-type")
    (assert (= (kw->header :host) "Host")                     "kw->header host")
    (assert (= (kw->header :x-custom-header) "X-Custom-Header")
      "kw->header x-custom-header")
    (assert (= (kw->header :content-length) "Content-Length")  "kw->header content-length")

    # round-trip
    (assert (= (kw->header (header->kw "Content-Type")) "Content-Type")
      "header round-trip Content-Type")
    (assert (= (kw->header (header->kw "Host")) "Host")
      "header round-trip Host")

    # read-headers via file transport
    (spit "/tmp/elle-http-test-headers"
      "Content-Type: text/plain\r\nHost: example.com\r\nContent-Length: 42\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-headers" :read
      (fn [t]
        (let [h (read-headers t)]
          (assert (= h:content-type   "text/plain")  "read-headers content-type")
          (assert (= h:host           "example.com") "read-headers host")
          (assert (= h:content-length "42")           "read-headers content-length"))))

    # read-headers trims whitespace
    (spit "/tmp/elle-http-test-headers-ws" "X-Foo:   bar baz   \r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-headers-ws" :read
      (fn [t]
        (let [h (read-headers t)]
          (assert (= h:x-foo "bar baz") "read-headers trims whitespace"))))

    # read-headers malformed
    (spit "/tmp/elle-http-test-headers-bad" "no-colon-here\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-headers-bad" :read
      (fn [t]
        (let [[ok? _] (protect (read-headers t))]
          (assert (not ok?) "read-headers malformed signals error"))))

    # write-headers
    (with-file-transport "/tmp/elle-http-test-write-headers" :write
      (fn [t]
        (write-headers t {:content-type "text/plain" :content-length "11"})
        (t-write t "\r\n")
        (t-flush t)))
    (let [content (slurp "/tmp/elle-http-test-write-headers")]
      (assert (string/contains? content "Content-Type: text/plain")
        "write-headers content-type")
      (assert (string/contains? content "Content-Length: 11")
        "write-headers content-length"))

    # read-request-line
    (spit "/tmp/elle-http-test-req-line" "GET /path HTTP/1.1\r\n")
    (with-file-transport "/tmp/elle-http-test-req-line" :read
      (fn [t]
        (let [rl (read-request-line t)]
          (assert (= rl:method  "GET")      "request-line method")
          (assert (= rl:path    "/path")    "request-line path")
          (assert (= rl:version "HTTP/1.1") "request-line version"))))

    # read-status-line
    (spit "/tmp/elle-http-test-status-200" "HTTP/1.1 200 OK\r\n")
    (with-file-transport "/tmp/elle-http-test-status-200" :read
      (fn [t]
        (let [sl (read-status-line t)]
          (assert (= sl:version "HTTP/1.1") "status-line version")
          (assert (= sl:status  200)         "status-line status")
          (assert (= sl:reason  "OK")        "status-line reason"))))

    # read-body with Content-Length
    (spit "/tmp/elle-http-test-body" "hello world")
    (with-file-transport "/tmp/elle-http-test-body" :read
      (fn [t]
        (let [body (read-body t {:content-length "11"})]
          (assert (= body "hello world") "read-body with content-length"))))

    # read-body without Content-Length
    (spit "/tmp/elle-http-test-body-no-cl" "ignored")
    (with-file-transport "/tmp/elle-http-test-body-no-cl" :read
      (fn [t]
        (let [body (read-body t {})]
          (assert (nil? body) "read-body without content-length is nil"))))

    # chunk-size: hex digits, with optional extension, error cases
    (assert (= (chunk-size "0")              0)   "chunk-size 0")
    (assert (= (chunk-size "a")              10)  "chunk-size a")
    (assert (= (chunk-size "1a")             26)  "chunk-size 1a")
    (assert (= (chunk-size "FF")             255) "chunk-size FF (uppercase)")
    (assert (= (chunk-size "10;ext=value")   16)  "chunk-size with extension")
    (assert (= (chunk-size "  20  ")         32)  "chunk-size trims whitespace")
    (let [[ok? _] (protect (chunk-size nil))]
      (assert (not ok?) "chunk-size nil signals error"))
    (let [[ok? _] (protect (chunk-size ""))]
      (assert (not ok?) "chunk-size empty signals error"))
    (let [[ok? _] (protect (chunk-size ";ext"))]
      (assert (not ok?) "chunk-size bare extension signals error"))

    # chunked? predicate
    (assert (chunked? {:transfer-encoding "chunked"})       "chunked? lowercase")
    (assert (chunked? {:transfer-encoding "Chunked"})       "chunked? mixed-case")
    (assert (chunked? {:transfer-encoding "gzip, chunked"}) "chunked? with gzip")
    (assert (not (chunked? {}))                             "chunked? absent")
    (assert (not (chunked? {:transfer-encoding "gzip"}))    "chunked? gzip-only")

    # read-chunked-body: happy path
    (spit "/tmp/elle-http-test-chunked"
      "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-chunked" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "hello world") "read-chunked-body concatenates chunks"))))

    # read-chunked-body: chunk extensions are ignored
    (spit "/tmp/elle-http-test-chunked-ext"
      "3;ext=foo\r\nabc\r\n0\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-chunked-ext" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "abc") "read-chunked-body ignores extensions"))))

    # read-chunked-body: trailers are discarded
    (spit "/tmp/elle-http-test-chunked-trail"
      "4\r\ndata\r\n0\r\nX-Trailer: value\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-chunked-trail" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "data") "read-chunked-body discards trailers"))))

    # read-chunked-body: hex sizes
    (spit "/tmp/elle-http-test-chunked-hex"
      "1a\r\nabcdefghijklmnopqrstuvwxyz\r\n0\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-chunked-hex" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "abcdefghijklmnopqrstuvwxyz")
            "read-chunked-body hex-encoded size"))))

    # read-chunked-body: empty body (just the 0-chunk)
    (spit "/tmp/elle-http-test-chunked-empty" "0\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-chunked-empty" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "") "read-chunked-body empty body"))))

    # read-body dispatches on Transfer-Encoding before Content-Length
    (spit "/tmp/elle-http-test-body-chunked"
      "5\r\nhello\r\n0\r\n\r\n")
    (with-file-transport "/tmp/elle-http-test-body-chunked" :read
      (fn [t]
        (let [body (read-body t {:transfer-encoding "chunked"
                                  :content-length "999"})]
          (assert (= body "hello")
            "read-body prefers chunked over content-length"))))

    # write-chunk: produces hex-size + CRLF + data + CRLF
    (with-file-transport "/tmp/elle-http-test-write-chunk" :write
      (fn [t]
        (write-chunk t "hi")
        (write-chunk t "there")
        (write-last-chunk t)
        (t-flush t)))
    (let [content (slurp "/tmp/elle-http-test-write-chunk")]
      (assert (= content "2\r\nhi\r\n5\r\nthere\r\n0\r\n\r\n")
        "write-chunk + write-last-chunk produce correct framing"))

    # write-chunk: empty chunk is a no-op
    (with-file-transport "/tmp/elle-http-test-write-chunk-empty" :write
      (fn [t]
        (write-chunk t "")
        (write-last-chunk t)
        (t-flush t)))
    (let [content (slurp "/tmp/elle-http-test-write-chunk-empty")]
      (assert (= content "0\r\n\r\n")
        "write-chunk empty is a no-op"))

    # round-trip: write chunks, read them back
    (with-file-transport "/tmp/elle-http-test-chunk-roundtrip" :write
      (fn [t]
        (write-chunk t "one ")
        (write-chunk t "two ")
        (write-chunk t "three")
        (write-last-chunk t)
        (t-flush t)))
    (with-file-transport "/tmp/elle-http-test-chunk-roundtrip" :read
      (fn [t]
        (let [body (read-chunked-body t)]
          (assert (= body "one two three") "chunk round-trip"))))

    # full request parse
    (spit "/tmp/elle-http-test-full-req"
      "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
    (let [out @[nil nil nil nil]]
      (with-file-transport "/tmp/elle-http-test-full-req" :read
        (fn [t]
          (let [rl (read-request-line t)]
            (put out 0 rl:method)
            (put out 1 rl:path)
            (let [h (read-headers t)]
              (put out 2 h:host)
              (let [body (read-body t h)]
                (put out 3 body))))))
      (assert (= (get out 0) "POST")      "full req method")
      (assert (= (get out 1) "/submit")   "full req path")
      (assert (= (get out 2) "localhost") "full req host")
      (assert (= (get out 3) "data")      "full req body"))

    # https URL without :tls plugin: clear error
    (when (nil? tls)
      (let [[ok? err] (protect (http-get "https://example.com/"))]
        (assert (not ok?) "https without tls signals error")
        (assert (= err:reason :tls-not-configured)
          "https without tls reports :tls-not-configured")))

    # query-encode: scalars
    (assert (= (query-encode {}) "")
      "query-encode empty → empty string")
    (assert (= (query-encode {:page 2}) "page=2")
      "query-encode integer value")
    (assert (= (query-encode {:q "hello world"}) "q=hello%20world")
      "query-encode percent-encodes spaces")
    (assert (= (query-encode {:flag true}) "flag=true")
      "query-encode boolean true")
    (assert (= (query-encode {:flag false}) "flag=false")
      "query-encode boolean false")

    # query-encode: keyword values render as bare strings (no colon)
    (assert (= (query-encode {:sort :asc}) "sort=asc")
      "query-encode keyword strips colon")

    # query-encode: nil values are dropped
    (assert (= (query-encode {:a 1 :b nil :c 3})
               "a=1&c=3")
      "query-encode omits nil values")

    # query-encode: arrays/lists produce repeated keys
    (assert (= (query-encode {:tag ["a" "b" "c"]})
               "tag=a&tag=b&tag=c")
      "query-encode repeats array values")

    # query-encode: reserved characters in keys and values are encoded
    (assert (= (query-encode {"a b" "c&d=e"}) "a%20b=c%26d%3De")
      "query-encode encodes reserved characters in both keys and values")

    # merge-query: struct merges with existing URL query
    (assert (= (merge-query "fmt=json" {:page 2}) "fmt=json&page=2")
      "merge-query appends struct to url query")
    (assert (= (merge-query nil {:page 2}) "page=2")
      "merge-query with nil url query")
    (assert (= (merge-query "fmt=json" nil) "fmt=json")
      "merge-query with nil extra keeps url query")
    (assert (= (merge-query "fmt=json" "page=2") "fmt=json&page=2")
      "merge-query accepts pre-encoded string")
    (assert (nil? (merge-query nil nil))
      "merge-query nil+nil is nil")

    # strip-line-terminator: parity between tcp and tls transports.
    # port/read-line strips newlines; tls:read-line does not; tls-transport
    # reconciles them so wire-format helpers see a single semantics.
    (assert (= (strip-line-terminator "hi\r\n") "hi") "strip-line-terminator CRLF")
    (assert (= (strip-line-terminator "hi\n")   "hi") "strip-line-terminator LF")
    (assert (= (strip-line-terminator "hi\r")   "hi") "strip-line-terminator CR")
    (assert (= (strip-line-terminator "hi")     "hi") "strip-line-terminator no terminator")
    (assert (= (strip-line-terminator "")       "")   "strip-line-terminator empty")
    (assert (nil? (strip-line-terminator nil))        "strip-line-terminator nil passthrough")

    # redirect-limit normalization
    (assert (= (redirect-limit nil)   0)  "redirect-limit nil → 0")
    (assert (= (redirect-limit false) 0)  "redirect-limit false → 0")
    (assert (= (redirect-limit true)  (default-redirect-limit))
      "redirect-limit true → default")
    (assert (= (redirect-limit 3)     3)  "redirect-limit integer passthrough")
    (let [[ok? _] (protect (redirect-limit "bogus"))]
      (assert (not ok?) "redirect-limit rejects non-integer/non-bool"))

    # resolve-location
    (let [base (parse-url "http://example.com/foo")]
      (assert (= (resolve-location base "https://other.com/bar")
                 "https://other.com/bar")
        "resolve-location absolute URL passthrough")
      (assert (= (resolve-location base "//other.com/bar")
                 "http://other.com/bar")
        "resolve-location scheme-relative")
      (assert (= (resolve-location base "/new/path")
                 "http://example.com:80/new/path")
        "resolve-location absolute path"))

    # redirect-statuses / get-rewrite-statuses
    (each s [301 302 303 307 308]
      (assert (redirect-statuses s)
        (string/format "status {} is a redirect" s)))
    (assert (not (redirect-statuses 200))  "200 is not a redirect")
    (assert (not (redirect-statuses 500))  "500 is not a redirect")
    (each s [301 302 303]
      (assert (get-rewrite-statuses s)
        (string/format "status {} rewrites to GET" s)))
    (each s [307 308]
      (assert (not (get-rewrite-statuses s))
        (string/format "status {} preserves method" s)))

    # SSE: field parsing
    (let [f (sse-parse-field "event: update")]
      (assert (= f:field "event") "sse-parse-field event name"))
    (let [f (sse-parse-field "data: hello")]
      (assert (= f:value "hello")
        "sse-parse-field eats one leading space on value"))
    (let [f (sse-parse-field "data:hello")]
      (assert (= f:value "hello")
        "sse-parse-field no space is fine too"))
    (assert (nil? (sse-parse-field ": comment"))
      "sse-parse-field treats : prefix as comment")
    (assert (nil? (sse-parse-field ""))
      "sse-parse-field skips empty line")
    (let [f (sse-parse-field "field-no-colon")]
      (assert (= f:field "field-no-colon")
        "sse-parse-field colonless line is field")
      (assert (= f:value "")
        "sse-parse-field colonless value is empty"))

    # SSE: sse-handle-line dispatches events on blank lines
    (let [events @[]
          state  @{:event-type nil :data-lines @[] :last-id nil :retry nil}]
      (defn collect [e] (push events e))
      (sse-handle-line state "event: ping"    collect)
      (sse-handle-line state "data: one"      collect)
      (sse-handle-line state "data: two"      collect)
      (sse-handle-line state "id: 42"         collect)
      (sse-handle-line state ""               collect)   # dispatch
      (sse-handle-line state "data: alone"    collect)
      (sse-handle-line state ""               collect)   # dispatch (no event, inherits id)
      (assert (= (length events) 2)        "sse-handle-line: 2 events")
      (let [e0 (get events 0)]
        (assert (= e0:event "ping")         "SSE event name")
        (assert (= e0:data  "one\ntwo")     "SSE multi-line data joined")
        (assert (= e0:id    "42")           "SSE id captured"))
      (let [e1 (get events 1)]
        (assert (= e1:event "message")      "SSE default event type")
        (assert (= e1:id    "42")           "SSE id persists across events")))

    # SSE: format-sse-event round-trips basics
    (assert (= (format-sse-event {:event "message" :data "hi"})
               "data: hi\n\n")
      "format-sse-event default event skips 'event:' line")
    (assert (= (format-sse-event {:event "tick" :data "1" :id "a"})
               "event: tick\nid: a\ndata: 1\n\n")
      "format-sse-event full frame")
    (assert (= (format-sse-event {:data "line1\nline2"})
               "data: line1\ndata: line2\n\n")
      "format-sse-event splits multi-line data")
    (assert (= (format-sse-event {:retry 5000})
               "retry: 5000\n\n")
      "format-sse-event retry-only event")

    # SSE: parse + format round-trip
    (let [events @[]
          state  @{:event-type nil :data-lines @[] :last-id nil :retry nil}
          wire   (format-sse-event {:event "tick" :data "hello" :id "7"})]
      (each line in (string/split wire "\n")
        (sse-handle-line state line (fn [e] (push events e))))
      (assert (= (length events) 1) "SSE round-trip: one event")
      (let [e (get events 0)]
        (assert (= e:event "tick")   "SSE round-trip: event")
        (assert (= e:data  "hello")  "SSE round-trip: data")
        (assert (= e:id    "7")      "SSE round-trip: id")))

    # SSE: sse-response builds a chunked streaming response
    (let [resp (sse-response (fn [send]
                                (send {:data "first"})
                                (send {:event "tick" :data "1"})))]
      (assert (= resp:status 200)
        "sse-response: status 200")
      (assert (= (get resp:headers :content-type) "text/event-stream")
        "sse-response: content-type")
      (assert (string/contains?
                (string/lowercase (get resp:headers :transfer-encoding))
                "chunked")
        "sse-response: transfer-encoding chunked")
      (assert (fn? resp:body) "sse-response: body is a closure"))

    true)

  ## ── Exports ─────────────────────────────────────────────────────────

  {:parse-url         parse-url

   # Header helpers
   :header->kw        header->kw
   :kw->header        kw->header

   # Response construction
   :respond           http-respond

   # Query-string encoding
   :query-encode     query-encode

   # Chunked transfer encoding
   :chunked?          chunked?
   :write-chunk       write-chunk
   :write-last-chunk  write-last-chunk

   # Compress helpers (require :compress at module init)
   :gzip              compress-gzip
   :gunzip            compress-gunzip
   :zlib              compress-zlib
   :unzlib            compress-unzlib
   :deflate           compress-deflate
   :inflate           compress-inflate
   :zstd              compress-zstd
   :unzstd            compress-unzstd

   # Transport helpers (for advanced users)
   :tcp-transport     tcp-transport
   :tls-transport     tls-transport

   # Client — one-shot
   :get               http-get
   :post              http-post
   :request           http-request

   # Client — keep-alive
   :connect           http-connect
   :send              http-send
   :close             http-close

   # Server
   :serve             http-serve

   # Server-Sent Events
   :sse-get           sse-get
   :sse-post          sse-post
   :sse-response      sse-response
   :format-sse-event  format-sse-event

   # Internal tests
   :test              run-internal-tests})
