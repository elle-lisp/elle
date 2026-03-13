## lib/http.lisp — Pure Elle HTTP/1.1 client and server
##
## Loaded via: (def http ((import-file "./lib/http.lisp")))
## Usage:      (http:get "http://example.com/")

# ============================================================================
# Internal helpers
# ============================================================================

(defn merge-structs [base override]
  "Merge two immutable structs. Keys in override win."
  (let ((result (@struct)))
    (each k in (keys base)
      (put result k (get base k)))
    (each k in (keys override)
      (put result k (get override k)))
    result))

# ============================================================================
# Chunk 1: URL parsing
# ============================================================================

(defn parse-url [url]
  "Parse an HTTP URL string into {:scheme :host :port :path :query}.
   Only 'http' scheme is supported. Default port is 80. Default path is '/'.
   Query is nil if absent, otherwise the string after '?' without the '?'."
  (when (not (string-starts-with? url "http://"))
    (error {:error :http-error :message (string/format "parse-url: unsupported scheme in: {}" url)}))
  (let ((rest (slice url 7 (length url))))   # strip "http://"
    (when (= (length rest) 0)
      (error {:error :http-error :message "parse-url: missing host"}))
    # Split host+port from path. Find first '/'.
    (let ((slash-pos (string/find rest "/")))
      (let ((authority (if (nil? slash-pos)
                          rest
                          (slice rest 0 slash-pos)))
            (path-and-query (if (nil? slash-pos)
                               "/"
                               (slice rest slash-pos (length rest)))))
        # Split authority into host and optional port.
        (let ((colon-pos (string/find authority ":")))
          (let ((host (if (nil? colon-pos)
                         authority
                         (slice authority 0 colon-pos)))
                (port (if (nil? colon-pos)
                         80
                         (integer (slice authority (+ colon-pos 1) (length authority))))))
            (when (= (length host) 0)
              (error {:error :http-error :message "parse-url: empty host"}))
            # Split path from query string.
            (let ((q-pos (string/find path-and-query "?")))
              (let ((path (if (nil? q-pos)
                             path-and-query
                             (slice path-and-query 0 q-pos)))
                    (query (if (nil? q-pos)
                              nil
                              (slice path-and-query (+ q-pos 1) (length path-and-query)))))
                {:scheme "http"
                 :host   host
                 :port   port
                 :path   path
                 :query  query}))))))))

# ============================================================================
# Module export closure
# ============================================================================

(fn []
  {:parse-url parse-url})
