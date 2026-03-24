## lib/aws.lisp — Elle-native AWS client

(var sigv4-mod nil)

(def creds {:access-key    (sys/env "AWS_ACCESS_KEY_ID")
            :secret-key    (sys/env "AWS_SECRET_ACCESS_KEY")
            :session-token (sys/env "AWS_SESSION_TOKEN")})
(def default-region (or (sys/env "AWS_DEFAULT_REGION")
                        (sys/env "AWS_REGION")
                        "us-east-1"))

(defn header-name [kw]
  (string/join (map (fn [part]
                      (if (empty? part) part
                        (concat (string/uppercase (first part))
                                (rest part))))
                    (string/split (string kw) "-"))
               "-"))

(defn decode-body [body headers]
  (if (nil? body)
    nil
    (let [[ct (or (get headers :content-type) "")]]
      (cond
        ((or (string-contains? ct "json")
             (string-contains? ct "x-amz-json"))
         (json/parse (string body)))
        ((or (string-contains? ct "text/")
             (string-contains? ct "xml"))
         (string body))
        (true body)))))

## ── Response reading (split out to keep function bodies small) ───────

(defn read-response [tls conn raw]
  ## Read status
  (let* [[line (tls:read-line conn)]
         [parts (string/split (string/trim line) " ")]
         [status (integer (get parts 1))]]
    ## Read headers
    (def resp-headers @{})
    (forever
      (let [[hline (tls:read-line conn)]]
        (when (or (nil? hline) (= (string/trim hline) ""))
          (break nil))
        (let [[colon (string/find hline ":")]]
          (when (not (nil? colon))
            (put resp-headers
                 (keyword (string/lowercase (slice hline 0 colon)))
                 (string/trim (slice hline (inc colon))))))))
    ## Read body
    (let* [[te (get resp-headers :transfer-encoding)]
           [cl (get resp-headers :content-length)]
           [resp-body
            (cond
              ((and te (string-contains? (string/lowercase te) "chunked"))
               (block :chunked
                 (def chunks @[])
                 (forever
                   (let* [[sz-line (tls:read-line conn)]
                          [sz (integer (string/trim sz-line) 16)]]
                     (when (= sz 0)
                       (tls:read-line conn)
                       (break :chunked
                         (if (empty? chunks)
                           (bytes)
                           (apply concat chunks))))
                     (let [[chunk (tls:read conn sz)]]
                       (push chunks chunk)
                       (tls:read-line conn))))))
              ((not (nil? cl))
               (tls:read conn (integer cl)))
              (true nil))]
           [decoded (if raw resp-body (decode-body resp-body resp-headers))]]
      {:status status :headers resp-headers :body decoded})))

## ── Request sending + connection ────────────────────────────────────

(defn aws-request-impl [tls service method path opts]
  (let* [[region   (or (get opts :region) default-region)]
         [host     (concat (string service) "." region ".amazonaws.com")]
         [body     (get opts :body)]
         [query    (get opts :query)]
         [req-path (if (nil? query) path (concat path "?" query))]
         [signed   (sigv4-mod:sign method path nil body host creds region
                                   (string service))]
         [headers  (if (nil? (get opts :headers))
                     signed
                     (merge signed (get opts :headers)))]
         [headers  (if (nil? body)
                     headers
                     (merge headers
                            {:content-length
                             (string (if (string? body)
                                      (string/size-of body)
                                      (length body)))}))]]
    (let [[conn (tls:connect host 443)]]
      (defer (tls:close conn)
        (tls:write conn (concat method " " req-path " HTTP/1.1\r\n"))
        (each [key value] in (pairs headers)
          (tls:write conn (concat (header-name key) ": " value "\r\n")))
        (tls:write conn "Connection: close\r\n\r\n")
        (unless (nil? body) (tls:write conn body))
        (read-response tls conn (get opts :raw))))))

## ── Entry point ──────────────────────────────────────────────────────

(fn [crypto jiff tls-module]
  (def tls tls-module)
  (assign sigv4-mod ((import-file "lib/aws/sigv4.lisp") crypto jiff))

  {:request (fn [service method path & args]
    (aws-request-impl tls service method path (or (first args) {})))})
