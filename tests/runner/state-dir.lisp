(elle/epoch 18)
## audited: 2026-11-22
## Where `elle test` keeps its session DB when no `--db` names one: a state directory, never the cache a reboot may clear.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a runner that reads `ELLE_CACHE` first puts run history
## on whatever that variable points at. On a development box that is a tmpfs,
## so every run recorded before a reboot is gone, and the history is the whole
## point of the store.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (def passes (write-fixture dir "pass.lisp" "(assert true \"ok\")\n"))

  (defn run-derived [vars]
    "Run the passing fixture with no `--db`, so the runner derives the store
     itself. Every variable the derivation reads is cleared, then VARS, a
     struct of names to values, sets the ones the case is about."
    (elle-run ["test" passes]
              :env (merge (env-without "ELLE_STATE" "XDG_STATE_HOME"
                                       "ELLE_CACHE" "HOME" "RUST_MIN_STACK")
                          vars)))

  (defn store-at [r db]
    "Require a green run that left a database, a CAS and a scratch directory
     at DB."
    (let [home (path/parent db)]
      (assert (= r:exit 0) (string "the run should gate green; stderr: " r:err))
      (assert (file/file? db)
              (string "expected the session DB at " db "; stderr: " r:err))
      (assert (file/directory? (path/join home "cas"))
              (string "the CAS belongs beside the DB, in " home))
      (assert (file/directory? (path/join home "scratch"))
              (string "the scratch directory belongs beside the DB, in " home))))

  (defn xdg-default [home]
    (path/join home ".local" "state" "elle" "elle-tests.db"))

  (eprintln "scenario: ELLE_STATE names the store")
  (let [state (path/join dir "state")]
    (store-at (run-derived {"ELLE_STATE" state})
              (path/join state "elle-tests.db")))

  (eprintln "scenario: XDG_STATE_HOME gets an elle directory")
  (let [xdg (path/join dir "xdg")]
    (store-at (run-derived {"XDG_STATE_HOME" xdg})
              (path/join xdg "elle" "elle-tests.db")))

  (eprintln "scenario: HOME falls back to the XDG default")
  (let [home (path/join dir "home")]
    (store-at (run-derived {"HOME" home}) (xdg-default home)))

  # A state variable outranks the cache, and the cache keeps nothing.
  (eprintln "scenario: the cache does not hold run history")
  (let [home (path/join dir "home-and-cache")
        cache (path/join dir "cache")]
    (file/mkdir-all cache)
    (store-at (run-derived {"HOME" home "ELLE_CACHE" cache}) (xdg-default home))
    (assert (not (file-exists? (path/join cache "elle-tests.db")))
            (string "the cache must hold no run history, found one in " cache)))

  # With no state variable and no home, the cache is the last place left that
  # names a directory.
  (eprintln "scenario: the cache is the last resort")
  (let [cache (path/join dir "cache-only")]
    (store-at (run-derived {"ELLE_CACHE" cache})
              (path/join cache "elle-tests.db")))

  # An empty variable names no directory, so the derivation reads past it. A
  # runner that took it literally would write to `/elle-tests.db`.
  (eprintln "scenario: an empty variable counts as unset")
  (let [home (path/join dir "home-empty")]
    (store-at (run-derived {"ELLE_STATE" "" "XDG_STATE_HOME" "" "HOME" home})
              (xdg-default home)))

  (eprintln "all state-dir scenarios passed"))
