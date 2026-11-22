(elle/epoch 18)
## audited: 2026-11-22
## What the runtime writes for a person to read: `debug/print`, `trace`, and no unresolved name on any surface a program reaches.
## tests/cli/overview.md
## docs/impl/symbol.md
##
## A render that threads no symbol table spells a symbol `#<symbol:hash>` and
## a keyword `#<keyword:hash>`. Those forms are unreadable on purpose, which
## makes them a canary: one in user-facing output means a render dropped the
## memo. The subject is what lands on the terminal rather than what a primitive
## returns, so these start the binary.

(include-file "../runner/lib/harness.lisp")

(defn stderr-of [input]
  "The stderr of INPUT, a program that must run cleanly on stdin. A failed
   run never reaches the print under test, and an empty stderr explains
   nothing about why."
  (let [r (elle-feed [] input)]
    (assert (= r:exit 0) (string input " must run cleanly; stderr: " r:err))
    r:err))

(defn names [input needle what]
  (let [err (stderr-of input)]
    (assert (string/contains? err needle) (string what ": " err))))

# The counter-factual: probe with a keyword the static vocabulary already
# spells, such as `:timeout` or `:ok`, and it resolves whether or not the
# print threads the memo. Only a spelling this instance learned from source
# separates the two.
(eprintln "scenario: debug/print names a keyword it prints")
(names "(debug/print :probe-debug-keyword)\n" ":probe-debug-keyword"
       "debug/print must name the keyword it prints")

# A symbol has no vocabulary behind it, so an unthreaded print hashes each one.
(eprintln "scenario: debug/print names a symbol it prints")
(names "(debug/print (quote probe-debug-symbol))\n" "probe-debug-symbol"
       "debug/print must name the symbol it prints")

# The trap: `trace` resolves its label through the memo already, so a check of
# the label alone passes while the value beside it reads `#<keyword:hash>`.
(eprintln "scenario: trace names the keyword it traces")
(names "(trace \"probe-label\" :probe-trace-keyword)\n" ":probe-trace-keyword"
       "trace must name the keyword it traces")

# Threading the memo into the outer render but not its recursion leaves every
# nested name hashed.
(eprintln "scenario: trace names a keyword nested in the traced value")
(names "(trace \"probe-label\" [1 :probe-nested-keyword])\n"
       ":probe-nested-keyword" "trace must name a keyword inside its value")

# Every surface a program's own names reach, in one program: the value the REPL
# echoes, `print`, `string`, the two diagnostic prints, the message of a type
# error, and the report of an error that reaches the root. The last form
# raises, so the run's status says nothing and both streams are read.
(eprintln "scenario: unresolved names reach no user-facing surface")
(let [r (elle-feed []
                   (string ":canary-echoed\n"
                           "(print [:canary-printed (quote canary-printed-sym)])\n"
                           "(print (string {:canary-stringified 1}))\n"
                           "(debug/print :canary-debugged)\n"
                           "(trace \"canary\" :canary-traced)\n"
                           "(print (protect ((quote canary-uncallable) 1)))\n"
                           "(error {:code :canary-raised :at (quote canary-raised-sym)})\n"))]
  (each [stream text] [["stdout" r:out] ["stderr" r:err]]
    (each form ["#<keyword:" "#<symbol:"]
      (assert (not (string/contains? text form))
              (string stream " carries " form ", so a render dropped the "
                      "symbol memo: " text))))
  # An empty stream carries no unresolved form either.
  (assert (and (string/contains? r:out "canary-printed")
               (string/contains? r:err "canary-raised"))
          (string "the probe must reach every surface it names: " r:out r:err)))

(eprintln "all diagnostics scenarios passed")
