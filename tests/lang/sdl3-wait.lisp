(elle/epoch 14)
# audited: 2026-09-30
## sdl/wait-event-timeout waits the seconds it is given when no event comes, and refuses a negative wait.
## lib/sdl3/window.lisp
##
## Only SDL's event subsystem starts, so no display is needed. Nothing sends
## an event, so each wait lasts its full bound. The file is gated on
## libSDL3, which a build machine may not have.
##
## SDL reads a negative wait as no bound, so a wrapper that passed -1
## through would never return. The refusal is checked last, after the
## assertions that fail fast on such a wrapper.

(def sdl
  (let [r (protect ((import "std/sdl3")))]
    (gate! (get r 0) "sdl3-wait: FFI or libSDL3 is unavailable" (get r 1))))

(sdl:init :events true)

(defn timed-wait [seconds]
  "Wait for an event for `seconds`. Returns [event elapsed-seconds]."
  (let* [started (clock/monotonic)
         got (sdl:wait-event-timeout seconds)]
    [got (- (clock/monotonic) started)]))

(defn assert-waited [seconds]
  "Assert that a wait of `seconds` returned no event, no sooner than that."
  (let [[got elapsed] (timed-wait seconds)]
    (assert (nil? got)
            (concat (string seconds) " s: nothing sends, got " (string got)))
    (assert (>= elapsed seconds)
            (concat (string seconds) " s: returned after " (string elapsed) " s"))))

## ── 1. a fraction of a second ────────────────────────────────────────

(assert-waited 0.2)

(println "  1. a wait of 0.2 s lasts 0.2 s")

## ── 2. a whole second ────────────────────────────────────────────────

(assert-waited 1)

(println "  2. a wait of 1 s lasts 1 s")

## ── 3. a negative wait is refused ────────────────────────────────────

(let [[ok? err] (protect (sdl:wait-event-timeout -1))]
  (assert (not ok?) "-1 s: expected :argument-error, got a return")
  (assert (= (get err :error) :argument-error)
          (concat "-1 s: expected :argument-error, got " (string err))))

(println "  3. a negative wait is refused")

(sdl:quit)

(println "sdl3-wait: all tests passed")
