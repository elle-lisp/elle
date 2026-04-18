(elle/epoch 8)
## tests/elle/aws.lisp — Elle-native AWS client test
##
## Requires AWS credentials in env:
##   AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN (optional)
##
## Run:
##   ./target/debug/elle tests/elle/aws.lisp

# Load plugins
(def crypto (import-file "target/debug/libelle_crypto.so"))
(def jiff   (import-file "target/debug/libelle_jiff.so"))
(def tls-p  (import-file "target/debug/libelle_tls.so"))
(def tls    ((import-file "lib/tls.lisp") tls-p))

# Load aws module + generated s3 layer
(def aws ((import-file "lib/aws.lisp") crypto jiff tls))
(def s3  ((import-file "lib/aws/s3.lisp") aws))

(println "listing buckets via aws:request...")
  (def result (aws:request :s3 "GET" "/"))
  (println "  status: " result:status)
  (println "  content-type: " (get result:headers :content-type))
  (println "  body type: " (type result:body))
  (assert (= result:status 200) "s3 GET / should return 200")

  # ── Generated S3 module ────────────────────────────────────────────

  (println "\nlisting via s3:list-buckets...")
  (def result2 (s3:list-buckets))
  (assert (= result2:status 200) "s3:list-buckets should return 200")
  (println "  status: " result2:status)

  # ── ListObjectsV2 ─────────────────────────────────────────────────

  (def test-bucket (sys/env "ELLE_TEST_BUCKET"))
  (when test-bucket
    (println "\nlisting objects in " test-bucket "...")
    (def result3 (s3:list-objects-v2 test-bucket {:max-keys "10"}))
    (println "  status: " result3:status)
    (assert (= result3:status 200) "list-objects-v2 should return 200")

    # ── Put/Get round-trip ───────────────────────────────────────────

    (def test-key (concat "elle-test/" (string (timestamp)) ".txt"))
    (def test-data "hello from elle-aws codegen")

    (println "\nput/get round-trip: " test-bucket "/" test-key)

    (def put-result (s3:put-object test-bucket test-key {:body test-data}))
    (println "  put status: " put-result:status)
    (assert (= put-result:status 200) "put-object should return 200")

    (def get-result (s3:get-object test-bucket test-key {:raw true}))
    (println "  get status: " get-result:status)
    (assert (= get-result:status 200) "get-object should return 200")
    (assert (= (string get-result:body) test-data) "round-trip data should match")
    (println "  round-trip OK")

    (def del-result (s3:delete-object test-bucket test-key))
    (println "  delete status: " del-result:status))

  (println "\nall aws tests passed")
