(elle/epoch 14)
# audited: 2026-10-06
## The aws module and the generated S3 module load with no credentials.

(def crypto (import "plugin/crypto"))
(def jiff (import "plugin/jiff"))
(def tls-p (import "plugin/tls"))
(def tls ((import "std/tls") tls-p))

(println "loading aws module...")
(def aws ((import "std/aws") crypto jiff tls))
(assert (not (nil? aws:request)) "aws:request should exist")
(println "  ok")

(println "loading generated s3 module...")
(def [ok? result] (protect (import "std/aws/s3")))
(if ok?
  (begin
    (println "  imported, initializing...")
    (def s3 (result aws))
    (println "  ok (" (length (keys s3)) " exports, api-version " s3:api-version
             ")"))
  (println "  ERROR: " result))

(println "done")
