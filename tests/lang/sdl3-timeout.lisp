(elle/epoch 14)
# audited: 2026-09-30
## sdl/wait-event-timeout gives SDL its seconds as whole milliseconds, rounded up, and refuses a wait SDL cannot count.
## lib/sdl3/window.lisp
##
## The conversion lives in std/sdl3/event, which loads on every build, with
## or without libSDL3 and FFI. sdl3-wait.lisp drives a real wait.
##
## The counter-factual is a ceiling of seconds × 1000. In binary floating
## point, 2.007 × 1000 is 2007.0000000000002, and its ceiling waits 2008 ms.

(def event ((import "std/sdl3/event")))

(defn assert-ms [seconds expected]
  "Assert that a wait of `seconds` is `expected` whole milliseconds."
  (let [ms (event:timeout-ms seconds)]
    (assert (and (int? ms) (= ms expected))
            (concat (string seconds) " s: expected " (string expected)
                    " ms, got " (string ms)))))

(defn assert-refused [seconds kind]
  "Assert that a wait of `seconds` raises an error of `kind`."
  (let [[ok? err] (protect (event:timeout-ms seconds))]
    (assert (not ok?)
            (concat (string seconds) " s: expected " (string kind) ", got "
                    (string err)))
    (assert (= (get err :error) kind)
            (concat (string seconds) " s: expected " (string kind) ", got "
                    (string err)))))

## ── 1. a whole number of milliseconds ────────────────────────────────

(assert-ms 0 0)
(assert-ms 2 2000)
(assert-ms 0.5 500)
(assert-ms 0.25 250)

(println "  1. a whole number of milliseconds converts exactly")

## ── 2. a fraction of a millisecond rounds up ─────────────────────────

(assert-ms 0.0004 1)
(assert-ms 0.0015 2)
(assert-ms 0.000000001 1)

(println "  2. a fraction of a millisecond rounds up")

## ── 3. a decimal the float cannot hold exactly ───────────────────────

(assert-ms 2.007 2007)
(assert-ms 4.001 4001)

(println "  3. a decimal the float cannot hold exactly is not rounded past")

## ── 4. the longest wait SDL counts ───────────────────────────────────

(assert-ms 2147483.647 2147483647)

(println "  4. the longest wait SDL counts converts")

## ── 5. a wait SDL cannot count is refused ────────────────────────────

(assert-refused -1 :argument-error)
(assert-refused -0.001 :argument-error)
(assert-refused 2147483.648 :argument-error)
(assert-refused (/ 1.0 0.0) :argument-error)
(assert-refused (/ 0.0 0.0) :argument-error)
(assert-refused "1" :type-error)
(assert-refused nil :type-error)

(println "  5. a wait SDL cannot count is refused")

(println "sdl3-timeout: all tests passed")
