(elle/epoch 16)
# audited: 2026-11-13
# import emits a request the nearest handler answers: a fiber whose mask names :import, import/with-loader, or the scheduler.
# docs/modules.md

(def here (path/parent (get (meta/location) :file)))

(defn echo-loader [request]
  "A loader that answers each request with the request itself."
  request)

# ── The request is a signal ─────────────────────────────────────────────

(assert (= 20 (get (signals) :import)) "the registry names the import bit")
(assert (macro? import) "import is a macro")

# A fiber whose mask names :import catches the request, and answers it as a
# join does, with [ok? value]. The counter-factual import loaded a file and
# emitted nothing.
(let [f (fiber/new (fn [] (+ 1 (import "./no-such-module"))) |:import|)
      request (fiber/resume f)]
  (assert (= :paused (fiber/status f)) "the import waits for an answer")
  (assert (= {:spec "./no-such-module" :dir here :loader import/load} request)
          "the request holds the spec, the writer's directory and the loader")
  (assert (= 42 (fiber/resume f [true 41]))
          "an answer of [true v] makes v the import's value"))

# The counter-factual answered the pair itself as the import's value.
(let [f (fiber/new (fn [] (protect (import "x"))) |:import|)]
  (fiber/resume f)
  (let [[ok? err] (fiber/resume f [false {:error :refused-here :message "no"}])]
    (assert (not ok?) "an answer of [false e] fails the import")
    (assert (= :refused-here (get err :error)) "and raises e")))

(let [f (fiber/new (fn [] (import "std/base64")) |:import :error|
                   :deny |:import|)
      denial (fiber/resume f)]
  (assert (= :capability-denied (get denial :error))
          "a fiber denied :import cannot import")
  (assert (= |:import| (get denial :denied)) "the denial names the bit"))

# ── import/with-loader answers the requests of its thunk ───────────────

(let [m (import/with-loader (fn [request] (fn [] {:spec (get request :spec)}))
                            (fn [] ((import "std/nothing"))))]
  (assert (= "std/nothing" m:spec) "the loader's value is the import's value"))

(assert (= {:spec "../modules/test" :dir here :loader import/load}
           (import/with-loader echo-loader (fn [] (import "../modules/test"))))
        "the loader receives the request as import emits it")

(assert (= :inner (import/with-loader (fn [_] :outer)
                                      (fn []
                                        (import/with-loader (fn [_] :inner)
                                          (fn [] (import "x"))))))
        "the innermost handler answers")

(assert (= :outer (import/with-loader (fn [_] :outer)
                                      (fn []
                                        (import/with-loader (fn [_] :inner)
                                          (fn [] 1))
                                        (import "x"))))
        "a handler answers only while its thunk runs")

(assert (= :answered (import/with-loader (fn [_] :answered)
                       (fn [] (get (protect (import "x")) 1))))
        "a request climbs past protect to the handler")

(let [[ok? err] (protect (import/with-loader
                           (fn [_]
                             (error {:error :loader-said-no :message "no"}))
                           (fn [] (import "x"))))]
  (assert (not ok?) "a loader's error fails the import")
  (assert (= :loader-said-no (get err :error))
          "and is the error the import raises"))

(assert (= ["a" "b"]
           (import/with-loader (fn [request] (get request :spec))
                               (fn [] (map (fn [s] (import s)) ["a" "b"]))))
        "import inside a function imports each spec of a list")

# ── The scheduler answers a program's requests ─────────────────────────

(assert (= import/load (*loader*)) "the parameter holds the default loader")

(let [m ((import "../modules/test"))]
  (assert (= 42 m:test-var) "the scheduler loads through the default loader"))

# importer.lisp imports ./test from inside its function. The call comes from
# this file, and the spec still resolves beside importer.lisp, which wrote it.
(let [importer ((import "../modules/importer"))
      m (importer:load)]
  (assert (= 42 m:test-var)
          "a spec resolves against the file that wrote it, not the caller's"))

# The loader is the one *loader* holds where the import runs. The
# counter-factual kept the loader each fiber was spawned under, so a
# parameterize around the import itself did not reach it.
(parameterize ((*loader* (fn [_] :here)))
  (assert (= :here (import "std/anything"))
          "a parameterize around an import picks its loader"))

(parameterize ((*loader* (fn [_] :counted)))
  (assert (= :counted (ev/join (ev/spawn (fn [] (import "std/anything")))))
          "a spawned fiber starts with the loader its spawner held"))

(let [f (parameterize ((*loader* (fn [_] :at-spawn)))
          (ev/spawn (fn [] (import "std/anything"))))]
  (parameterize ((*loader* (fn [_] :at-join)))
    (assert (= :at-spawn (ev/join f))
            "a parameterize in another fiber does not reach the importer")))

(let [f (import/with-loader (fn [_] :handler)
                            (fn [] (ev/spawn (fn [] (import "../modules/test")))))]
  (let [m ((ev/join f))]
    (assert (= 42 m:test-var)
            "a spawned fiber's request goes to the scheduler, not the spawner's handler")))

(parameterize ((*loader* (fn [_] (error {:error :loader-said-no :message "no"}))))
  (let [[ok? err] (ev/join-protected (ev/spawn (fn [] (import "x"))))]
    (assert (not ok?) "a loader's error fails the spawned import")
    (assert (= :loader-said-no (get err :error))
            "and the importer raises the loader's error")))

# The loader runs on a fiber of its own, so it may wait as any fiber does. The
# counter-factual ran the loader on the scheduler's fiber, where a join has no
# scheduler to park on.
(let [ticker (ev/spawn (fn []
                         (ev/sleep 0.01)
                         :ticked))]
  (parameterize ((*loader* (fn [_] (ev/join ticker))))
    (assert (= :ticked (ev/join (ev/spawn (fn [] (import "x")))))
            "a loader waits on another fiber while the importer waits on it")))

# An importer aborted while its loader runs is not resumed when the loader
# ends, and the loader still runs to its end. The counter-factual resumed the
# dead importer, which raises out of the event loop. Each wait below polls a
# flag, so no clock decides the order.
(let [log @[]
      release @[false]]
  (parameterize ((*loader* (fn [_]
                             (while (not (get release 0)) (ev/sleep 0.001))
                             (push log :loaded)
                             :late)))
    (let [importer (ev/spawn (fn [] (push log (import "x"))))]
      (ev/sleep 0)
      (ev/abort importer)
      (put release 0 true)
      (while (empty? log) (ev/sleep 0.001))
      (ev/sleep 0)
      (assert (= [:loaded] (freeze log))
              "the loader ran to its end and the aborted importer saw nothing"))))

# ── A process's scheduler answers its sub-fibers' requests ─────────────

# lib/process runs a process's sub-fibers on a scheduler of its own, which
# answers a request as ev/run's does. The counter-factual kept the loader each
# sub-fiber was spawned under there too.
(def process ((import "std/process")))
(def answers @[])
(process:start (fn []
                 (push answers
                       (parameterize ((*loader* (fn [_] :spawned)))
                         (ev/join (ev/spawn (fn [] (import "x"))))))
                 (push answers
                       (ev/join-protected
                         (ev/spawn (fn []
                                     (parameterize ((*loader* (fn [_] :inner)))
                                       (import "x"))))))
                 (push answers
                       (ev/join-protected
                         (ev/spawn (fn []
                                     (parameterize ((*loader*
                                                      (fn [_]
                                                        (error
                                                          {:error :loader-said-no
                                                           :message "no"}))))
                                       (import "x"))))))))
(assert (= :spawned (get answers 0))
        "a sub-fiber starts with the loader its spawner held")
(assert (= [true :inner] (get answers 1))
        "a sub-fiber's parameterize around its import picks the loader")
(assert (= :loader-said-no (get (get (get answers 2) 1) :error))
        "a loader's error fails the sub-fiber's import")

# ── A compile cannot import ─────────────────────────────────────────────

(let [[ok? err] (protect (eval '(begin
                                  (defmacro m []
                                    (begin
                                      (import "std/base64")
                                      1))
                                  (m))))]
  (assert (not ok?) "an import in a macro body fails the compile")
  (assert (string/contains? (get err :message) ":import")
          "and the error names the bit the expansion withholds"))
