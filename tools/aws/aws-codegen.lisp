## tools/aws/aws-codegen.lisp — Generate Elle API module from AWS Smithy model
##
## Usage:
##   elle tools/aws/aws-codegen.lisp -- s3          > lib/aws/s3.lisp
##   elle tools/aws/aws-codegen.lisp -- dynamodb    > lib/aws/dynamodb.lisp
##   elle tools/aws/aws-codegen.lisp -- lambda      > lib/aws/lambda.lisp
##   elle tools/aws/aws-codegen.lisp -- sts         > lib/aws/sts.lisp
##
## Supports protocols: restXml, restJson1, awsJson1_0, awsJson1_1, awsQuery, ec2Query
##
## Fetch models:
##   elle tools/aws/fetch-model.lisp -- s3 dynamodb lambda sts

# ── Helpers (must be defined before use) ─────────────────────────────

(defn strip-ns [name]
  (def idx (string/find name "#"))
  (if idx (slice name (+ idx 1)) name))

(defn camel->kebab [s]
  (def out @"")
  (var i 0)
  (while (< i (length s))
    (def ch (get s i))
    (def upper (and (>= ch "A") (<= ch "Z")))
    (when (and upper (> i 0))
      (def prev (get s (- i 1)))
      (def prev-upper (and (>= prev "A") (<= prev "Z")))
      (if prev-upper
        (when (and (< (+ i 1) (length s))
                   (let [[next (get s (+ i 1))]]
                     (and (>= next "a") (<= next "z"))))
          (push out "-"))
        (push out "-")))
    (push out (string/lowercase ch))
    (assign i (+ i 1)))
  (string out))

(defn parse-uri [uri]
  (def qidx (string/find uri "?"))
  (def path-part (if qidx (slice uri 0 qidx) uri))
  (def fixed-query (if qidx (slice uri (+ qidx 1)) nil))
  (def fixed-query
    (when fixed-query
      (def parts (filter (fn [p] (not (string/starts-with? p "x-id=")))
                         (string/split fixed-query "&")))
      (if (empty? parts) nil (string/join parts "&"))))
  {:path path-part :fixed-query fixed-query})

(def open-brace (string (bytes 123)))
(def close-brace (string (bytes 125)))

(defn emit-line [& parts]
  (if (empty? parts)
    (print "\n")
    (print (apply concat (map string parts)) "\n")))

(defn indent [n & parts]
  (print (string/join (map (fn [_] " ") (range 0 (* n 2))) ""))
  (apply emit-line parts))

(defn build-path-expr [path-template labels]
  (def parts @[])
  (var remaining path-template)
  (forever
    (def open (string/find remaining "{"))
    (when (nil? open)
      (when (not (= remaining ""))
        (push parts (concat "\"" remaining "\"")))
      (break nil))
    (def prefix (slice remaining 0 open))
    (when (not (= prefix ""))
      (push parts (concat "\"" prefix "\"")))
    (def close (string/find remaining "}"))
    (def label-raw (slice remaining (+ open 1) close))
    (def label (if (string/ends-with? label-raw "+")
                 (slice label-raw 0 (- (length label-raw) 1))
                 label-raw))
    (push parts (camel->kebab label))
    (assign remaining (slice remaining (+ close 1))))
  (if (= (length parts) 1)
    (first parts)
    (concat "(concat " (string/join parts " ") ")")))

# ── Args + model loading ─────────────────────────────────────────────

(def user-args (drop 1 (sys/args)))
(when (empty? user-args)
  (eprintln "usage: elle tools/aws/aws-codegen.lisp -- <service>")
  (eprintln "       e.g. s3, dynamodb, lambda, ec2, sqs, sns, sts, iam")
  (error {:error :usage :message "missing service name"}))

(def service (first user-args))
(def model-path (concat "aws-models/" service ".json"))

(def [ok? raw] (protect (begin
  (def p (port/open model-path :read))
  (def chunks @[])
  (forever
    (def chunk (port/read p 65536))
    (when (or (nil? chunk) (= (length chunk) 0))
      (break nil))
    (push chunks chunk))
  (port/close p)
  (apply concat chunks))))

(when (not ok?)
  (eprintln "error: could not read " model-path)
  (eprintln "fetch it with: elle tools/aws/fetch-model.lisp -- " service)
  (error {:error :io :message (concat "missing model: " model-path)}))

(def model (json/parse (string raw)))
(def shapes (get model "shapes"))

# ── Detect protocol ──────────────────────────────────────────────────

(var protocol nil)
(var json-version nil)
(var target-prefix nil)
(var api-version nil)

(each [name shape] in (pairs shapes)
  (when (= (get shape "type") "service")
    (def traits (or (get shape "traits") {}))
    (assign target-prefix (strip-ns name))
    (assign api-version (get shape "version"))
    (cond
      ((has? traits "aws.protocols#restJson1")   (assign protocol :rest-json))
      ((has? traits "aws.protocols#restXml")     (assign protocol :rest-xml))
      ((has? traits "aws.protocols#awsJson1_0")
       (assign protocol :aws-json)
       (assign json-version "1.0"))
      ((has? traits "aws.protocols#awsJson1_1")
       (assign protocol :aws-json)
       (assign json-version "1.1"))
      ((has? traits "aws.protocols#awsQuery")    (assign protocol :aws-query))
      ((has? traits "aws.protocols#ec2Query")    (assign protocol :aws-query))
      (true nil))))

(when (nil? protocol)
  (eprintln "error: could not detect protocol for " service)
  (error {:error :protocol :message "unknown protocol"}))

# ── Member classifiers ───────────────────────────────────────────────

(defn classify-members [input-shape-name]
  (def shape (get shapes input-shape-name))
  (def members (or (get shape "members") {}))
  (def req-set @||)
  (def result @{:labels    @[]
                :queries   @[]
                :headers   @[]
                :payload   nil
                :required  req-set})
  (each [name member] in (pairs members)
    (def traits (or (get member "traits") {}))
    (when (has? traits "smithy.api#required")
      (add (get result :required) name))
    (cond
      ((has? traits "smithy.api#httpLabel")
       (push (get result :labels) name))
      ((has? traits "smithy.api#httpQuery")
       (push (get result :queries)
             {:name name :query-key (get traits "smithy.api#httpQuery")}))
      ((has? traits "smithy.api#httpHeader")
       (push (get result :headers)
             {:name name :header-name (get traits "smithy.api#httpHeader")}))
      ((has? traits "smithy.api#httpPayload")
       (put result :payload name))
      (true nil)))
  result)

(defn collect-input-members [input-shape-name]
  (def shape (get shapes input-shape-name))
  (def members (or (get shape "members") {}))
  (def result @[])
  (each [name member] in (pairs members)
    (def traits (or (get member "traits") {}))
    (def json-name (or (get traits "smithy.api#jsonName") name))
    (push result {:name      name
                  :json-name json-name
                  :required  (has? traits "smithy.api#required")}))
  result)

# ── REST protocol emitter (restXml, restJson1) ───────────────────────

(defn emit-rest-function [op-name op-shape]
  (def traits (get op-shape "traits"))
  (def http (get traits "smithy.api#http"))
  (when (nil? http) nil)
  (def method (get http "method"))
  (def parsed-uri (parse-uri (get http "uri")))

  (def input-ref (get op-shape "input"))
  (when (nil? input-ref) nil)
  (def members (classify-members (get input-ref "target")))

  (def fn-name (camel->kebab (strip-ns op-name)))
  (def labels (get members :labels))
  (def queries (get members :queries))
  (def headers (get members :headers))
  (def payload (get members :payload))
  (def required (get members :required))

  (def positional @[])
  (each l in labels (push positional l))
  (each q in queries
    (when (contains? required q:name) (push positional q:name)))

  (emit-line)
  (def all-params @[])
  (each p in positional (push all-params (camel->kebab p)))
  (push all-params "&keys")
  (push all-params "opts")
  (def param-list (string/join all-params " "))
  (indent 1 "(defn " fn-name " [" param-list "]")
  (indent 2 "(let* [[opts (or opts {})]")

  (indent 5 "[path " (build-path-expr (get parsed-uri :path) labels) "]")

  (def has-queries (not (empty? queries)))
  (def has-fixed-query (not (nil? (get parsed-uri :fixed-query))))
  (when (or has-queries has-fixed-query)
    (indent 5 "[query-parts @[]]"))

  (indent 5 "[req-headers {}]")
  (indent 5 "[body " (if payload
                        (concat "(get opts :" (camel->kebab payload) ")")
                        "nil") "]")
  (emit-line "             ]")

  (when has-fixed-query
    (indent 3 "(push query-parts \"" (get parsed-uri :fixed-query) "\")"))

  (each q in queries
    (def elle-name (camel->kebab q:name))
    (def is-positional (contains? required q:name))
    (def var-ref (if is-positional elle-name
                   (concat "(get opts :" elle-name ")")))
    (if is-positional
      (indent 3 "(push query-parts (concat \"" q:query-key "=\" (string " var-ref ")))")
      (indent 3 "(when " var-ref
              " (push query-parts (concat \"" q:query-key "=\" (string " var-ref "))))")))

  (each h in headers
    (def elle-name (camel->kebab h:name))
    (indent 3 "(when (get opts :" elle-name ")"
            " (assign req-headers (merge req-headers"
            " {:" (string/lowercase h:header-name) " (string (get opts :" elle-name "))})))"))

  (when (or has-queries has-fixed-query)
    (indent 3 "(def query-str (if (empty? query-parts) nil"
            " (string/join query-parts \"&\")))"))

  (def query-ref (if (or has-queries has-fixed-query) "query-str" "nil"))
  (indent 3 "(request \"" method "\" path"
          " (merge opts " open-brace ":query " query-ref
          " :headers req-headers :body body" close-brace "))))"))

# ── awsJson protocol emitter ─────────────────────────────────────────

(defn emit-json-function [op-name op-shape]
  (def input-ref (get op-shape "input"))
  (when (nil? input-ref) nil)
  (def members (collect-input-members (get input-ref "target")))
  (def op-short (strip-ns op-name))
  (def fn-name (camel->kebab op-short))
  (def target-val (concat target-prefix "." op-short))

  (def positional (filter (fn [m] m:required) members))
  (def optional (filter (fn [m] (not m:required)) members))

  (emit-line)
  (def all-params @[])
  (each m in positional (push all-params (camel->kebab m:name)))
  (push all-params "&keys")
  (push all-params "opts")
  (def param-list (string/join all-params " "))
  (indent 1 "(defn " fn-name " [" param-list "]")
  (indent 2 "(def opts (or opts {}))")
  (indent 2 "(def body @{})")

  (each m in positional
    (indent 2 "(put body \"" m:json-name "\" " (camel->kebab m:name) ")"))
  (each m in optional
    (def elle-name (camel->kebab m:name))
    (indent 2 "(when (get opts :" elle-name ")"
            " (put body \"" m:json-name "\" (get opts :" elle-name ")))"))

  (indent 2 "(request \"POST\" \"/\""
          " " open-brace ":headers " open-brace
          ":x-amz-target \"" target-val "\""
          " :content-type \"application/x-amz-json-" (or json-version "1.0") "\""
          close-brace " :body (json/stringify body)" close-brace "))"))

# ── awsQuery protocol emitter ────────────────────────────────────────

(defn emit-query-function [op-name op-shape]
  (def input-ref (get op-shape "input"))
  (when (nil? input-ref) nil)
  (def members (collect-input-members (get input-ref "target")))
  (def op-short (strip-ns op-name))
  (def fn-name (camel->kebab op-short))

  (def positional (filter (fn [m] m:required) members))
  (def optional (filter (fn [m] (not m:required)) members))

  (emit-line)
  (def all-params @[])
  (each m in positional (push all-params (camel->kebab m:name)))
  (push all-params "&keys")
  (push all-params "opts")
  (def param-list (string/join all-params " "))
  (indent 1 "(defn " fn-name " [" param-list "]")
  (indent 2 "(def opts (or opts {}))")
  (indent 2 "(def parts @[\"Action=" op-short "\"])")

  (each m in positional
    (indent 2 "(push parts (concat \"" m:name "=\" (string " (camel->kebab m:name) ")))"))
  (each m in optional
    (def elle-name (camel->kebab m:name))
    (indent 2 "(when (get opts :" elle-name ")"
            " (push parts (concat \"" m:name "=\" (string (get opts :" elle-name ")))))"))

  (indent 2 "(request \"POST\" \"/\""
          " " open-brace ":headers " open-brace
          ":content-type \"application/x-www-form-urlencoded\""
          close-brace " :body (string/join parts \"&\")" close-brace "))"))

# ── Main: collect ops, emit module ───────────────────────────────────

(var ops @[])
(each [name shape] in (pairs shapes)
  (when (= (get shape "type") "operation")
    (push ops [name shape])))
(assign ops (sort ops))

# For REST protocols, only count ops with HTTP bindings
(var emitted-names @[])
(each [name shape] in ops
  (cond
    ((or (= protocol :rest-xml) (= protocol :rest-json))
     (def traits (or (get shape "traits") {}))
     (when (has? traits "smithy.api#http")
       (push emitted-names (camel->kebab (strip-ns name)))))
    (true
     (push emitted-names (camel->kebab (strip-ns name))))))

(eprintln service ": " (length emitted-names) " operations (" (string protocol) ")")

(emit-line "## lib/aws/" service ".lisp — Generated " service " API from Smithy model")
(emit-line "## API version: " api-version)
(emit-line "## DO NOT EDIT — regenerate with:")
(emit-line "##   elle tools/aws/aws-codegen.lisp -- " service " > lib/aws/" service ".lisp")
(emit-line)
(emit-line "(fn [aws]")
(emit-line "  (def request (partial aws:request :" service "))")

(each [name shape] in ops
  (cond
    ((or (= protocol :rest-xml) (= protocol :rest-json))
     (def traits (or (get shape "traits") {}))
     (when (has? traits "smithy.api#http")
       (emit-rest-function name shape)))
    ((= protocol :aws-json)
     (emit-json-function name shape))
    ((= protocol :aws-query)
     (emit-query-function name shape))
    (true nil)))

(emit-line)
(emit-line "  # ── Module exports ─────────────────────────────────────────────")
(emit-line "  " open-brace)
(indent 2 ":api-version \"" api-version "\"")
(each fn-name in emitted-names
  (indent 2 ":" fn-name " " fn-name))
(emit-line "  " close-brace ")")
