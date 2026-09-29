(elle/epoch 13)
# audited: 2026-09-28
## HTTP/1.1 client and server over TCP, with HTTPS and compression as module arguments.
## lib/http.md
## lib/http/overview.md
##
## Plain HTTP only:
##   (def http ((import "std/http")))
##
## HTTPS client support requires the std/tls module, built from the tls
## plugin, passed as :tls:
##   (def tls ((import "std/tls") (import "plugin/tls")))
##   (def http ((import "std/http") :tls tls))
##
## Usage: (http:get "http://example.com/")   (http:get "https://...")

(fn [&named tls compress]

  ## ── Submodules ───────────────────────────────────────────────────────

  (def url ((import "std/http/url")))
  (def transport ((import "std/http/transport") :tls tls))
  (def chunked ((import "std/http/chunked") :transport transport))
  (def wire ((import "std/http/wire") :transport transport :chunked chunked))
  (def sse
    ((import "std/http/sse") :url url :transport transport :wire wire
                             :chunked chunked))

  ## The client and server below call these by their short names.
  (def parse-url url:parse-url)
  (def merge-query url:merge-query)
  (def open-transport transport:open)
  (def tcp-transport transport:tcp)
  (def t-write transport:write)
  (def t-flush transport:flush)
  (def t-close transport:close)
  (def write-request-line wire:write-request-line)
  (def write-headers wire:write-headers)
  (def build-request-headers wire:build-request-headers)
  (def read-request-line wire:read-request-line)
  (def read-status-line wire:read-status-line)
  (def write-status-line wire:write-status-line)
  (def read-headers wire:read-headers)
  (def read-body wire:read-body)
  (def reason-phrases wire:reason-phrases)
  (def chunked? chunked:chunked?)
  (def write-chunk chunked:write-chunk)
  (def write-last-chunk chunked:write-last-chunk)

  ## ── Optional compress plugin ─────────────────────────────────────────
  ## Accept either a pre-imported compress struct (from (import "std/compress"))
  ## or literal `true` to have this module import it for us. Exposes the
  ## standard compress helpers (gzip/gunzip/zlib/unzlib/deflate/inflate/
  ## zstd/unzstd) as http:<name>. No automatic Accept-Encoding negotiation —
  ## callers apply these explicitly to bodies or chunks.

  (def compress-mod
    (cond
      (nil? compress) nil
      (= compress true) ((import "std/compress"))
      (struct? compress) compress
      true (error {:error :http-error
                   :reason :bad-compress
                   :value compress
                   :message ":compress must be nil, true, or a compress module struct"})))

  (defn require-compress []
    "Return the configured compress module, or signal a clear error if
     :compress was not supplied at module init."
    (when (nil? compress-mod)
      (error {:error :http-error
              :reason :compress-not-configured
              :message ":compress option was not supplied to (import \"std/http\")"}))
    compress-mod)

  (defn compress-gzip [data & opts]
    (let [c (require-compress)]
      (apply c:gzip data opts)))
  (defn compress-gunzip [data]
    (let [c (require-compress)]
      (c:gunzip data)))
  (defn compress-zlib [data & opts]
    (let [c (require-compress)]
      (apply c:zlib data opts)))
  (defn compress-unzlib [data]
    (let [c (require-compress)]
      (c:unzlib data)))
  (defn compress-deflate [data & opts]
    (let [c (require-compress)]
      (apply c:deflate data opts)))
  (defn compress-inflate [data]
    (let [c (require-compress)]
      (c:inflate data)))
  (defn compress-zstd [data & opts]
    (let [c (require-compress)]
      (apply c:zstd data opts)))
  (defn compress-unzstd [data]
    (let [c (require-compress)]
      (c:unzstd data)))

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

  (defn default-redirect-limit []
    10)

  (defn resolve-location [base location]
    "Resolve a redirect Location header value against the base URL
     struct. Handles absolute URLs, scheme-relative ('//host/path'),
     and absolute paths ('/foo'). Other forms are treated as absolute
     paths rooted at '/'."
    (cond
      (or (string/starts-with? location "http://")
          (string/starts-with? location "https://")) location
      (string/starts-with? location "//") (string base:scheme ":" location)
      true
        (let [path (if (string/starts-with? location "/")
                     location
                     (string "/" location))]
          (string base:scheme "://" base:host ":" base:port path))))

  (defn redirect-limit [follow]
    "Normalize the :follow-redirects option: nil/false → 0, true →
     default, integer → itself."
    (cond
      (nil? follow) 0
      (= follow false) 0
      (= follow true) (default-redirect-limit)
      (integer? follow) follow
      true (error {:error :http-error
                   :reason :bad-follow-redirects
                   :value follow
                   :message ":follow-redirects must be nil, true, or a non-negative integer"})))

  ## ── Client API ───────────────────────────────────────────────────────

  (defn wants-close? [headers]
    "True if headers indicate the connection should close."
    (let [conn headers:connection]
      (and conn (= (string/lowercase conn) "close"))))

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
      (defer
        (protect (t-close t))
        (send-request t method request-path url-parsed:host headers body false))))

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
          (when (or (zero? remaining) (not (redirect-statuses resp:status)))
            (break :redirects nil))
          (let [loc (get resp:headers :location)]
            (when (nil? loc) (break :redirects nil))
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
    (send-request session:transport method path session:host headers body true))

  (defn http-close [session]
    "Close a keep-alive session."
    (t-close session:transport))

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
            (nil? body) nil
            (fn? body)
              (body (fn [data] (write-chunk t data)))
            true (write-chunk t body))
          (write-last-chunk t))
      (not (nil? response:body)) (t-write t response:body))
    (t-flush t))

  (defn connection-loop [t handler on-error]
    "Handle HTTP requests on a transport until it closes or either side
     sends Connection: close. Errors in handler produce a 500 response.
     on-error is called with (request error) when the handler fails."
    (defer
      (protect (t-close t))
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
            (when (or (wants-close? req:headers) (wants-close? response:headers))
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
     their own transport."
    (default on-error default-on-error)
    (forever
      (let [[ok? conn] (protect (tcp/accept listener))]
        (unless ok? (break))
        (ev/spawn (fn [] (connection-loop (tcp-transport conn) handler on-error))))))

  ## ── Internal tests ──────────────────────────────────────────────────

  (defn run-internal-tests []
    "Run every submodule's checks, then the client's own. Called via
     (http:test)."
    (url:test)
    (transport:test)
    (chunked:test)
    (wire:test)
    (sse:test)

    # https URL without :tls plugin: clear error
    (when (nil? tls)
      (let [[ok? err] (protect (http-get "https://example.com/"))]
        (assert (not ok?) "https without tls signals error")
        (assert (= err:reason :tls-not-configured)
                "https without tls reports :tls-not-configured")))

    # redirect-limit normalization
    (assert (= (redirect-limit nil) 0) "redirect-limit nil → 0")
    (assert (= (redirect-limit false) 0) "redirect-limit false → 0")
    (assert (= (redirect-limit true) (default-redirect-limit))
            "redirect-limit true → default")
    (assert (= (redirect-limit 3) 3) "redirect-limit integer passthrough")
    (let [[ok? _] (protect (redirect-limit "bogus"))]
      (assert (not ok?) "redirect-limit rejects non-integer/non-bool"))

    # resolve-location
    (let [base (parse-url "http://example.com/foo")]
      (assert (= (resolve-location base "https://other.com/bar")
                 "https://other.com/bar")
              "resolve-location absolute URL passthrough")
      (assert (= (resolve-location base "//other.com/bar")
                 "http://other.com/bar") "resolve-location scheme-relative")
      (assert (= (resolve-location base "/new/path")
                 "http://example.com:80/new/path")
              "resolve-location absolute path"))

    # redirect-statuses / get-rewrite-statuses
    (each s [301 302 303 307 308]
      (assert (redirect-statuses s) (string/format "status {} is a redirect" s)))
    (assert (not (redirect-statuses 200)) "200 is not a redirect")
    (assert (not (redirect-statuses 500)) "500 is not a redirect")
    (each s [301 302 303]
      (assert (get-rewrite-statuses s)
              (string/format "status {} rewrites to GET" s)))
    (each s [307 308]
      (assert (not (get-rewrite-statuses s))
              (string/format "status {} preserves method" s)))
    true)

  ## ── Exports ─────────────────────────────────────────────────────────

  {:parse-url parse-url

   # Header helpers
   :header->kw wire:header->kw
   :kw->header wire:kw->header

   # Response construction
   :respond http-respond

   # Query-string encoding
   :query-encode url:query-encode

   # Chunked transfer encoding
   :chunked? chunked?
   :write-chunk write-chunk
   :write-last-chunk write-last-chunk

   # Compress helpers (require :compress at module init)
   :gzip compress-gzip
   :gunzip compress-gunzip
   :zlib compress-zlib
   :unzlib compress-unzlib
   :deflate compress-deflate
   :inflate compress-inflate
   :zstd compress-zstd
   :unzstd compress-unzstd

   # Transport helpers (for advanced users)
   :tcp-transport tcp-transport
   :tls-transport transport:tls

   # Client — one-shot
   :get http-get
   :post http-post
   :request http-request

   # Client — keep-alive
   :connect http-connect
   :send http-send
   :close http-close

   # Server
   :serve http-serve

   # Server-Sent Events
   :sse-get sse:get
   :sse-post sse:post
   :sse-response sse:response
   :format-sse-event sse:format-event

   # Internal tests
   :test run-internal-tests})
