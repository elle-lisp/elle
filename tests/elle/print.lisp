(elle/epoch 12)

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

# Scratch dir for the redirect fixture; removed at the bottom of the file.
(def scratch (file/mktempdir))
(def tmp-path (path/join scratch "redirect.txt"))
(def out (port/open tmp-path :write))
(parameterize ((*stdout* out))
  (println "captured line"))
(port/close out)

(def in (port/open tmp-path :read))
(def contents (port/read-all in))
(port/close in)
(assert (= (string contents) "captured line\n")
        "println respects *stdout* rebinding")

(file/delete-dir-all scratch)
(println "all print tests passed")
