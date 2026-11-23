(elle/epoch 18)
## audited: 2026-11-22
## A REPL def may call a function a later line defines; two defs that call each other do not compile.
## tests/cli/overview.md
## docs/bindings.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/repl.lisp")

(eprintln "scenario: a def calls a function a later line defines")
(echoes "(defn foo [] (bar))\n(defn bar [] 42)\n(foo)\n" ["42"]
        "a forward reference across lines")

# Each line compiles alone, and a name exists from its def onward, so a retry of
# either def still misses the other, and the session exits 1. The
# counter-factual is a retry that compiles the waiting defs together as one
# file: it fails the same way, since a file binds no name before its def. A
# retry that succeeded would echo :done and exit 0.
(eprintln "scenario: two defs that call each other stay unresolved")
(let [r (session (string "(defn ping [n] (if (= n 0) :done (pong (- n 1))))\n"
                         "(defn pong [n] (if (= n 0) :done (ping (- n 1))))\n"
                         "(ping 10)\n"))]
  (assert (= r:exit 1) (string "the session exits 1, got " r:exit))
  (assert (not (echoed? r ":done")) (string "no retry binds the pair: " r:out))
  (assert (and (string/contains? r:err "✗ ping: unresolved (pong undefined)")
               (string/contains? r:err "✗ pong: unresolved (ping undefined)"))
          (string "both defs stay unresolved: " r:err)))

(eprintln "scenario: a destructured letrec binds mutual recursion")
(echoes (string "(def [ping pong]\n"
                "  (letrec [ping (fn [n] (if (= n 0) :done (pong (- n 1))))\n"
                "           pong (fn [n] (if (= n 0) :done (ping (- n 1))))]\n"
                "    [ping pong]))\n(ping 10)\n") [":done"]
        "a letrec of the pair")

(eprintln "scenario: a forward reference on one line resolves")
(echoes "(defn foo [] (bar)) (defn bar [] 42) (foo)\n" ["42"]
        "a forward reference on one line")

(eprintln "scenario: a deferred def resolves once a later line defines its callee")
(echoes "(defn foo [x] (bar x))\n(defn bar [x] (* x 10))\n(foo 3)\n" ["30"]
        "a deferred def")

(eprintln "scenario: chained forward references resolve")
(echoes "(defn foo [] (bar))\n(defn bar [] (baz))\n(defn baz [] 99)\n(foo)\n"
        ["99"] "a chain of forward references")

(eprintln "all repl-forward scenarios passed")
