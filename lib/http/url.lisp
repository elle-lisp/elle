(elle/epoch 13)
# audited: 2026-09-28
## HTTP and HTTPS URL parsing, and query-string encoding.
## lib/http/overview.md
##
## Loaded via:
##   (def url ((import "std/http/url")))
##
## Exports: {:parse-url :query-encode :merge-query :test}

(fn []

  ## ── URL parsing ──────────────────────────────────────────────────────

  (def url-schemes
    {"http" {:prefix-len 7 :default-port 80}
     "https" {:prefix-len 8 :default-port 443}})

  (defn pick-scheme [url]
    "Return [scheme info] for url, or nil if no supported prefix matches."
    (cond
      (string/starts-with? url "https://") ["https" (get url-schemes "https")]
      (string/starts-with? url "http://") ["http" (get url-schemes "http")]
      true nil))

  (defn parse-url [url]
    "Parse an HTTP or HTTPS URL string into {:scheme :host :port :path :query}.
     Supports 'http' (default port 80) and 'https' (default port 443).
     Default path is '/'. Query is nil if absent, otherwise the string after '?'
     without the '?'."
    (let [picked (pick-scheme url)]
      (when (nil? picked)
        (error {:error :http-error
                :reason :unsupported-scheme
                :url url
                :message "unsupported scheme"}))
      (let* [[scheme info] picked
             tail (slice url info:prefix-len)
             slash (string/find tail "/")
             auth (if (nil? slash) tail (slice tail 0 slash))
             path+query (if (nil? slash) "/" (slice tail slash))
             colon (string/find auth ":")
             host (if (nil? colon) auth (slice auth 0 colon))
             port (if (nil? colon)
                    info:default-port
                    (parse-int (slice auth (inc colon))))]
        (when (empty? tail)
          (error {:error :http-error
                  :reason :missing-host
                  :url url
                  :message "missing host"}))
        (when (empty? host)
          (error {:error :http-error
                  :reason :empty-host
                  :url url
                  :message "empty host"}))
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
      true (string v)))

  (defn query-encode-pair [ekey v parts]
    "Push 'key=value' into parts (uri-encoded). Skip nil, recurse into
     arrays/lists to produce repeated 'key=v1&key=v2' pairs."
    (cond
      (nil? v) nil
      (or (array? v) (list? v))
        (each elt in v
          (unless (nil? elt)
            (push parts
                  (string/format "{}={}" ekey
                                 (uri-encode (query-scalar->string elt))))))
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
                    (nil? extra) nil
                    (string? extra) extra
                    true (query-encode extra))]
      (cond
        (and url-query encoded) (string url-query "&" encoded)
        true (or url-query encoded))))

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on query encoding."
    # query-encode: scalars
    (assert (= (query-encode {}) "") "query-encode empty → empty string")
    (assert (= (query-encode {:page 2}) "page=2") "query-encode integer value")
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
    (assert (= (query-encode {:a 1 :b nil :c 3}) "a=1&c=3")
            "query-encode omits nil values")

    # query-encode: arrays/lists produce repeated keys
    (assert (= (query-encode {:tag ["a" "b" "c"]}) "tag=a&tag=b&tag=c")
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
    (assert (nil? (merge-query nil nil)) "merge-query nil+nil is nil")
    true)

  {:parse-url parse-url
   :query-encode query-encode
   :merge-query merge-query
   :test run-tests})
