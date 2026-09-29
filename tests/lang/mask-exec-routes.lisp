(elle/epoch 12)
# audited: 2026-09-29
## Naming a capability in a fiber mask catches the operations that raise it, with no other bit required first.
## docs/signals/capabilities.md
##
## A mask catches a signal when the two share any bit, with no exception. A
## subprocess request is `|:io :exec|`, so a mask of `|:error :exec|` catches it
## on `:exec`.
##
## THE TRAP: an intermediate fiber masking `|:yield|` must not swallow a request
## the scheduler has to service. The source handles that: an I/O request raises
## `|:io|` without `:yield`, so a `|:yield|` mask does not overlap it at all. The
## generator case is pinned by tests/lang/io-request-carries-no-yield.lisp.
##
## COUNTER-FACTUAL: a `covers` that catches `:io` only for a mask that also
## names `:io` runs the subprocess under `|:error :exec|` and reports `:dead`
## with bits 0 — indistinguishable from a fiber that never spawned anything. A
## sandbox author who believed they were watching subprocess calls would be
## watching none.

# Naming :exec catches the subprocess request.
(let [f (fiber/new (fn []
                     (subprocess/system "echo" ["ran"])
                     :done) |:error :exec|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a mask naming :exec parks the fiber on a subprocess request")
  (assert (= (fiber/bits f) 2560)
          (string "the request is |:io :exec| — got " (fiber/bits f))))

# Naming :io catches the same request: both bits route, neither is privileged.
(let [f (fiber/new (fn []
                     (subprocess/system "echo" ["ran"])
                     :done) |:error :io|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a mask naming :io parks on the same request")
  (assert (= (fiber/bits f) 2560) "and sees the same bits"))

# `fiber/bits` still tells a subprocess request from a plain port read, which is
# what lets one `|:io|` mask audit both without denying either.
(let [plain (fiber/new (fn []
                         (ev/sleep 0.001)
                         :done) |:error :io|)]
  (fiber/resume plain)
  (assert (= (fiber/bits plain) 512)
          (string "a non-exec io request is |:io| alone — got "
                  (fiber/bits plain))))

# A mask naming neither bit catches nothing, and the request reaches the
# scheduler. This is the case the old exception was protecting, and it still
# holds — now because the signal and the mask genuinely share no bit.
(let [f (fiber/new (fn []
                     (ev/sleep 0.001)
                     :done) |:error :yield|)]
  (assert (= (fiber/resume f) :done)
          "a mask sharing no bit with the request lets it through")
  (assert (= (fiber/status f) :dead) "and the fiber runs to completion"))

(println "mask-exec-routes: ok")
