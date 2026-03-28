(elle/epoch 6)

# ── println / print ──────────────────────────────────────────────────

(println "hello from println")
(print "no newline: ")
(println "after print")
(println)

# ── eprintln / eprint ────────────────────────────────────────────────

(eprintln "this goes to stderr")
(eprint "stderr no newline: ")
(eprintln "after eprint")

# ── multiple args ────────────────────────────────────────────────────

(println "count: " 42 " done")
(eprintln "error code: " 99)

# ── *stdout* rebinding ──────────────────────────────────────────────

(def tmp-path "/tmp/elle-print-test-redirect.txt")
(def out (port/open tmp-path :write))
    (parameterize ((*stdout* out))
      (println "captured line"))
    (port/close out)

(def in (port/open tmp-path :read))
    (def contents (port/read-all in))
    (port/close in)
    (assert (= (string contents) "captured line\n") "println respects *stdout* rebinding")

(println "all print tests passed")
