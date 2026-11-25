(elle/epoch 18)
## audited: 2026-11-23
## The harness tests the `elle` beside the executable that runs it, and no environment variable moves it.
## tests/runner/overview.md
## tests/cli/overview.md
##
## The counter-factual: a harness that read `ELLE`, or preferred a release
## build, could run a debug rig's tests against another build's `elle` and
## report on a binary the run never named.

(include-file "lib/harness.lisp")

(def beside (path/absolute (path/join (path/parent (elle/executable)) "elle")))

(eprintln "scenario: the harness names the elle beside its executable")
(assert (= elle beside) (string "the harness runs " elle ", not " beside))

(with-temp-dir dir
  (eprintln "scenario: ELLE does not move the binary under test")
  (let [probe (write-fixture dir "probe.lisp"
                             (string "(include-file \""
                                     (path/absolute
                                       "tests/runner/lib/harness.lisp")
                                     "\")\n(println elle)\n"))
        r (subprocess/system (elle/executable) [probe]
                             {:env (put (sys/env) "ELLE"
                                        (path/join dir "no-such-elle"))})]
    (assert (= r:exit 0) (string "the probe failed: " r:stderr))
    (assert (= (string/trim r:stdout) beside)
            (string "with ELLE set, the harness runs " (string/trim r:stdout)))))

(eprintln "all harness scenarios passed")
