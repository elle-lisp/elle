(elle/epoch 8)
## tools/aws/fetch-model.lisp — Download AWS Smithy models via HTTPS
##
## Usage:
##   elle tools/aws/fetch-model.lisp -- s3 dynamodb lambda

(def tls-p  (import-file "target/debug/libelle_tls.so"))
(def tls    ((import-file "lib/tls.lisp") tls-p))

(defn https-get [host path]
  "GET an HTTPS resource, return body as bytes."
  (def conn (tls:connect host 443))
  (defer (tls:close conn)
    (tls:write conn (concat "GET " path " HTTP/1.1\r\n"
                            "Host: " host "\r\n"
                            "Connection: close\r\n\r\n"))
    # Read status
    (def line (tls:read-line conn))
    (def status (parse-int (get (string/split (string/trim line) " ") 1)))
    # Read headers
    (def headers @{})
    (forever
      (def hline (tls:read-line conn))
      (when (or (nil? hline) (= (string/trim hline) ""))
        (break nil))
      (def colon (string/find hline ":"))
      (when (not (nil? colon))
        (put headers
             (keyword (string/lowercase (slice hline 0 colon)))
             (string/trim (slice hline (+ colon 1))))))
    # Read body
    (def te (get headers :transfer-encoding))
    (def cl (get headers :content-length))
    (def body
      (cond
        ((and te (string-contains? (string/lowercase te) "chunked"))
         (def chunks @[])
         (forever
           (def sz (parse-int (string/trim (tls:read-line conn)) 16))
           (when (= sz 0)
             (tls:read-line conn)
             (break (if (empty? chunks) (bytes) (apply concat chunks))))
           (def chunk (tls:read conn sz))
           (push chunks chunk)
           (tls:read-line conn)))
        ((not (nil? cl))
         (tls:read conn (parse-int cl)))
        (true (tls:read-all conn))))
    {:status status :body body}))

# ── Main ─────────────────────────────────────────────────────────────

(def user-args (drop 1 (sys/args)))
(when (empty? user-args)
  (eprintln "usage: elle tools/aws/fetch-model.lisp -- <service> [service...]")
  (error {:error :usage :message "missing service name"}))

(each service in user-args
    (def path (concat "/awslabs/aws-sdk-rust/main/aws-models/" service ".json"))
    (def dest (concat "aws-models/" service ".json"))
    (eprintln service ": fetching...")
    (def result (https-get "raw.githubusercontent.com" path))
    (if (= result:status 200)
      (begin
        (def p (port/open dest :write))
        (port/write p result:body)
        (port/close p)
        (eprintln "  → " dest " (" (length result:body) " bytes)"))
      (eprintln "  FAILED (HTTP " result:status ")")))
