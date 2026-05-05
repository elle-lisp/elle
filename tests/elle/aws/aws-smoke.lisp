(elle/epoch 10)
## tests/elle/aws-smoke.lisp — verify aws module loads (no creds needed)

(def crypto (import-file "target/debug/libelle_crypto.so"))
(def jiff (import-file "target/debug/libelle_jiff.so"))
(def tls-p (import-file "target/debug/libelle_tls.so"))
(def tls ((import-file "lib/tls.lisp") tls-p))

(println "loading aws module...")
(def aws ((import-file "lib/aws.lisp") crypto jiff tls))
(assert (not (nil? aws:request)) "aws:request should exist")
(println "  ok")

(println "loading generated s3 module...")
(def [ok? result] (protect (import-file "lib/aws/s3.lisp")))
(if ok?
  (begin
    (println "  imported, initializing...")
    (def s3 (result aws))
    (println "  ok (" (length (keys s3)) " exports, api-version " s3:api-version
             ")"))
  (println "  ERROR: " result))

(println "done")
