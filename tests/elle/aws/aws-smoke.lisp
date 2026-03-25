(elle/epoch 6)
## tests/elle/aws-smoke.lisp — verify aws module loads (no creds needed)

(def crypto (import-native "crypto"))
(def jiff   (import-native "jiff"))
(def tls-p  (import-native "tls"))
(def tls    ((import "tls") tls-p))

(println "loading aws module...")
(def aws ((import "aws") crypto jiff tls))
(assert (not (nil? aws:request)) "aws:request should exist")
(println "  ok")

(println "loading generated s3 module...")
(def [ok? result] (protect (import "aws/s3")))
(if ok?
  (begin
    (println "  imported, initializing...")
    (def s3 (result aws))
    (println "  ok (" (length (keys s3)) " exports, api-version " s3:api-version ")"))
  (println "  ERROR: " result))

(println "done")
