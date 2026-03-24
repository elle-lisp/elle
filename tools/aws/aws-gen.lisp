## tools/aws/aws-gen.lisp — Fetch Smithy models and generate Elle API modules
##
## Usage:
##   elle tools/aws/aws-gen.lisp -- s3 dynamodb sts
##
## For each service:
##   1. Fetches aws-models/{service}.json if missing (via HTTPS)
##   2. Generates lib/aws/{service}.lisp if missing or model is newer
##
## To force regeneration, delete the generated file first.

(def tls-p  (import-file "target/debug/libelle_tls.so"))
(def tls    ((import-file "lib/tls.lisp") tls-p))

(defn https-get [host path]
  (def conn (tls:connect host 443))
  (defer (tls:close conn)
    (tls:write conn (concat "GET " path " HTTP/1.1\r\n"
                            "Host: " host "\r\n"
                            "Connection: close\r\n\r\n"))
    (def line (tls:read-line conn))
    (def status (integer (get (string/split (string/trim line) " ") 1)))
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
    (def te (get headers :transfer-encoding))
    (def cl (get headers :content-length))
    (def body
      (cond
        ((and te (string-contains? (string/lowercase te) "chunked"))
         (def chunks @[])
         (forever
           (def sz (integer (string/trim (tls:read-line conn)) 16))
           (when (= sz 0)
             (tls:read-line conn)
             (break (if (empty? chunks) (bytes) (apply concat chunks))))
           (def chunk (tls:read conn sz))
           (push chunks chunk)
           (tls:read-line conn)))
        ((not (nil? cl))
         (tls:read conn (integer cl)))
        (true (tls:read-all conn))))
    {:status status :body body}))

(def services (drop 1 (sys/args)))
(when (empty? services)
  (eprintln "usage: elle tools/aws/aws-gen.lisp -- <service> [service...]")
  (eprintln "       e.g. s3, dynamodb, lambda, ec2, sqs, sns, sts, iam")
  (error {:error :usage :message "missing service name"}))

(def elle-bin (or (sys/env "ELLE_BIN") "./target/debug/elle"))

(ev/run (fn []
  (each service in services
    (def model-path (concat "aws-models/" service ".json"))
    (def output-path (concat "lib/aws/" service ".lisp"))

    # ── Fetch model if missing ────────────────────────────────────────

    (var model-stat (file/stat model-path))
    (when (nil? model-stat)
      (eprintln service ": fetching model...")
      (def path (concat "/awslabs/aws-sdk-rust/main/aws-models/" service ".json"))
      (def result (https-get "raw.githubusercontent.com" path))
      (when (not (= result:status 200))
        (eprintln service ": fetch failed (HTTP " result:status ")")
        (break nil))
      (def p (port/open model-path :write))
      (port/write p result:body)
      (port/close p)
      (eprintln "  → " model-path " (" (length result:body) " bytes)")
      (assign model-stat (file/stat model-path)))

    # ── Generate if needed ────────────────────────────────────────────

    (def output-stat (file/stat output-path))
    (def needs-gen (or (nil? output-stat)
                       (> model-stat:modified output-stat:modified)))

    (if needs-gen
      (begin
        (def result (subprocess/system elle-bin
                      ["tools/aws/aws-codegen.lisp" "--" service]))
        (if (= result:exit 0)
          (begin
            (def p (port/open output-path :write))
            (port/write p (bytes result:stdout))
            (port/close p)
            (def new-stat (file/stat output-path))
            (eprintln service ": " output-path " (" new-stat:size " bytes)"
                      " " (string/trim result:stderr)))
          (eprintln service ": codegen failed\n" result:stderr)))
      (eprintln service ": up to date")))))
