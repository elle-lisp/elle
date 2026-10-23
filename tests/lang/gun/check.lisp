(elle/epoch 15)
# audited: 2026-10-21
# What every std/gun test file shares: the error a call raises, a frozen clock, the plugin gate, and errors nobody expected.
# lib/gun.md
# tests/lang/overview.md
#
# A file that includes this one binds gun, the module under test, before it
# calls anything here. A file is a letrec, so these bodies may name it.

(defn fails-with [reason thunk]
  "Run thunk, and answer the error it raises after checking its reason."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error with reason " reason))
    (assert (= (get err :error) :gun-error) "a gun error")
    (assert (= (get err :reason) reason) (string "the reason is " reason))
    err))

(defn frozen-clock [ms]
  "A clock whose wall clock stands still at ms."
  (gun:clock :now-ms (fn [] ms)))

# The import stays literal in the test file, because the plugin checks of the
# suite read each (import "plugin/X") there.
(defn plugin [name loaded]
  "The plugin that (protect (import \"plugin/NAME\")) loaded, or a gated skip
   of the whole file when it did not load."
  (let [[ok? p] loaded]
    (unless ok?
      (error {:error :gated :reason (string "plugin/" name " not available")}))
    p))

# Every database reports here unless a test watches its errors itself.
# all-passed checks that nothing arrived.
(def unexpected @[])

(defn open-db [&named @clock @wait @store @on-error @peers @redial @module]
  "A database through module, gun by default, whose errors go to unexpected
   unless :on-error takes them."
  (default on-error (fn [err] (push unexpected err)))
  (default module gun)
  (module:open :clock clock :wait wait :store store :on-error on-error
               :peers peers :redial redial))

(defn all-passed [name]
  "Check that no database reported an error nobody expected, and say that
   the file tests/lang/NAME.lisp passed."
  (assert (empty? unexpected)
          (string "no database reported an error: " unexpected))
  (println "tests/lang/" name ".lisp: all tests passed"))
