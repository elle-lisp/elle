(elle/epoch 18)
# audited: 2026-11-20
# Semantic portraits of a function or a module, built from the compile/analyze queries.
# docs/analysis/portrait.md
#
# Usage:
#   (def portrait ((import "std/portrait")))
#   (def a (compile/analyze (file/read "myfile.lisp") {:file "myfile.lisp"}))
#   (println (portrait:render (portrait:function a :my-fn)))
#   (println (portrait:render-module (portrait:module a)))

(fn []

  # ── Lint advisories ─────────────────────────────────────────────────────
  #
  # A portrait REFLECTS the linter's findings; it never re-derives them. Both
  # granularities read compile/diagnostics through the one accessor below, so
  # portrait and `elle lint` cannot disagree, and a new rule reaches both by
  # gaining a row in `lint-kinds`.

  (def lint-kinds
    [["mutable-binding-never-assigned" :false-mutable]
     ["unused-binding" :unused-binding]
     ["non-tail-self-recursion" :non-tail-recursion]])

  (defn diagnostics-for [analysis rule function]
    "The linter's `rule` diagnostics. With a `function` name, only those the
    linter attributed to that function; with nil, every one in the module."
    (filter (fn [d]
              (and (= (get d :rule) rule)
                   (or (nil? function) (= (get d :function) function))))
            (compile/diagnostics analysis)))

  (defn advisories-for [analysis rule kind]
    "Module-wide advisories for one rule, tagged with the portrait's `kind`."
    (freeze (map (fn [d]
                   {:kind kind :line (get d :line) :message (get d :message)})
                 (diagnostics-for analysis rule nil))))

  # ── Failure mode detection ──────────────────────────────────────────────

  (defn callee-raises? [callee bit]
    "Whether the signal a callee edge carries holds `bit`. A callee whose
    signal the analysis does not know answers false."
    (let [sig (get callee :signal)]
      (and (not (nil? sig)) (contains? (get sig :bits) bit))))

  (defn detect-failure-modes [callees]
    "The calls whose callee may raise an error."
    (freeze (map (fn [callee]
                   {:source (get callee :name)
                    :line (get callee :line)
                    :kind :error})
                 (filter (fn [callee] (callee-raises? callee :error)) callees))))

  # ── Composition assessment ──────────────────────────────────────────────

  (defn assess-composition [sig captures]
    "Determine composition properties from signal and captures."
    (let* [has-mutable-capture (not (empty?
                                      (filter (fn [c] (get c :mutated)) captures)))
           has-io (get sig :io)
           has-any-capture (not (empty? captures))]
      {:retry-safe (not has-io)
       :timeout-safe (not has-mutable-capture)
       :parallelizable (not has-mutable-capture)
       :memoizable (and (get sig :silent) (not has-any-capture))
       :stateless (empty? captures)}))

  # ── Observation engine ──────────────────────────────────────────────────

  (defn find-observations [analysis name sig captures callees]
    "Generate observations about non-obvious properties."
    (def @obs @[])

    # 1. Almost-pure: only one I/O callee
    (let [io-callees (filter (fn [c] (callee-raises? c :io)) callees)]
      (when (and (not (get sig :silent)) (= 1 (length io-callees)))
        (let [io-callee (first io-callees)]
          (push obs
                {:kind :almost-pure
                 :message (string/format
                            "Only I/O source is {} at line {}. Factoring it out leaves the rest free of I/O."
                            (get io-callee :name) (get io-callee :line))}))))

    # 2. Mutable capture shared across closures
    (each cap in captures
      (when (get cap :mutated)
        (let [captured-by (protect (compile/captured-by analysis
                                     (keyword (get cap :name))))]
          (when (and (get captured-by 0) (> (length (get captured-by 1)) 1))
            (push obs
                  {:kind :shared-mutable
                   :message (string "Mutable binding '" (get cap :name)
                                    "' is captured by "
                                    (length (get captured-by 1))
                                    " functions. Concurrent fibers will race.")})))))

    # 3. Unsandboxed delegation
    (when (not (empty? (get sig :propagates)))
      (each idx in (get sig :propagates)
        (push obs
              {:kind :unsandboxed-delegation
               :message (string "Parameter " idx
                                " is called without signal bounds."
                                " A malicious closure could do"
                                " arbitrary I/O, yield indefinitely,"
                                " or crash the caller."
                                " Consider (bound param ||)"
                                " or a fuel budget.")})))

    # 4. All-tail-call chain
    (when (and (not (empty? callees))
               (not (empty? (filter (fn [c] (get c :tail)) callees)))
               (= (length (filter (fn [c] (get c :tail)) callees))
                  (length callees)))
      (push obs
            {:kind :tail-chain
             :message "All calls are in tail position. This function is a state machine candidate."}))

    # 5. Capture-by-value of mutable source
    (each cap in captures
      (when (and (= (get cap :kind) :value) (not (get cap :mutated)))
        (let [binding-result (protect (compile/binding analysis
                                        (keyword (get cap :name))))]
          (when (and (get binding-result 0)
                     (get (get binding-result 1) :mutated))
            (push obs
                  {:kind :stale-capture
                   :message (string/format
                              "Captures '{}' by value, but '{}' is mutated elsewhere. This closure sees the value at capture time, not later mutations."
                              (get cap :name) (get cap :name))})))))

    # 6-8. Lint advisories raised inside THIS function. Attribution is by the
    # :function the linter stamped on each diagnostic, so a finding in a nested
    # closure belongs to the closure, not to its host.
    (each pair in lint-kinds
      (each d in (diagnostics-for analysis (get pair 0) (string name))
        (push obs {:kind (get pair 1) :message (get d :message)})))

    (freeze obs))

  # ── Function portrait ──────────────────────────────────────────────────

  (defn function-portrait [analysis name]
    "Build a semantic portrait of a named function."
    (let* [sig (compile/signal analysis name)
           caps (compile/captures analysis name)
           callers (compile/callers analysis name)
           callees (compile/callees analysis name)
           failures (detect-failure-modes callees)
           comp (assess-composition sig caps)
           obs (find-observations analysis name sig caps callees)]
      {:name (string name)
       :signal sig
       :captures caps
       :failures failures
       :composition comp
       :observations obs
       :callers callers
       :callees callees}))

  (defn false-mutable-advisories [analysis]
    "A binding declared mutable (var/@) yet never reassigned via `assign`."
    (advisories-for analysis "mutable-binding-never-assigned" :false-mutable))

  (defn unused-binding-advisories [analysis]
    "A binding that is defined and never read."
    (advisories-for analysis "unused-binding" :unused-binding))

  (defn non-tail-recursion-advisories [analysis]
    "A function whose self-call sits outside tail position."
    (advisories-for analysis "non-tail-self-recursion" :non-tail-recursion))

  # ── Module portrait ─────────────────────────────────────────────────────

  (defn module-portrait [analysis]
    "Build a signal topology for an entire module. Only the silent functions
    are sorted, under :pure; each function's signal says what the rest do."
    (let* [syms (compile/symbols analysis)
           fns (filter (fn [s] (= (get s :kind) :function)) syms)
           silent? (fn [name]
                     (let [r (protect (compile/signal analysis (keyword name)))]
                       (and (get r 0) (get (get r 1) :silent))))
           pure (freeze (filter silent? (map (fn [s] (get s :name)) fns)))]

      # Find signal boundaries
      (def @boundaries @[])
      (let [graph (compile/call-graph analysis)]
        (each node in (get graph :nodes)
          (let* [caller-name (get node :name)
                 caller-result (protect (compile/signal analysis
                                          (keyword caller-name)))]
            (when (get caller-result 0)
              (let [caller-sig (get caller-result 1)]
                (each callee-name in (get node :callees)
                  (let [callee-result (protect
                                        (compile/signal analysis
                                          (keyword callee-name)))]
                    (when (get callee-result 0)
                      (let [callee-sig (get callee-result 1)]
                        (when (not (= (get caller-sig :silent)
                                      (get callee-sig :silent)))
                          (push boundaries
                                {:caller caller-name
                                 :callee callee-name
                                 :transition (if (get caller-sig :silent)
                                               :pure-to-impure
                                               :impure-to-pure)})))))))))))

      {:pure pure
       :boundaries (freeze boundaries)
       :false-mutable (false-mutable-advisories analysis)
       :unused-binding (unused-binding-advisories analysis)
       :non-tail-recursion (non-tail-recursion-advisories analysis)
       :graph (compile/call-graph analysis)}))

  # ── Text rendering ──────────────────────────────────────────────────────

  (defn format-signal [sig]
    "Format a signal struct as a readable string."
    (let [bits (get sig :bits)]
      (if (empty? bits)
        (if (empty? (get sig :propagates))
          "silent"
          (string/format "propagates params {}" (get sig :propagates)))
        (string/join (map string (->list bits)) ", "))))

  (defn render-function [portrait]
    "Render a function portrait as text."
    (def @out @"")
    (let [name (get portrait :name)
          sig (get portrait :signal)
          caps (get portrait :captures)
          comp (get portrait :composition)]
      (push out (string/format "{}\n\n" name))
      (push out (string/format "  Effects:       {}\n" (format-signal sig)))

      (when (not (empty? caps))
        (push out
              (string/format "  Captures:      {}\n"
                             (string/join
                               (map (fn [c]
                                      (string/format "{} ({})" (get c :name)
                                        (get c :kind))) caps) ", "))))

      (when (empty? caps) (push out "  Captures:      none\n"))

      (when (not (empty? (get portrait :failures)))
        (push out "\n  Failure modes:\n")
        (each f in (get portrait :failures)
          (push out
                (string/format "    - {} at line {} ({})\n" (get f :source)
                               (get f :line) (get f :kind)))))

      (push out "\n  Composition:\n")
      (each [k v] in (pairs comp)
        (push out (string/format "    {}: {}\n" k v)))

      (when (not (empty? (get portrait :observations)))
        (push out "\n  Observations:\n")
        (each o in (get portrait :observations)
          (push out
                (string/format "    [{}] {}\n" (get o :kind) (get o :message))))))

    (freeze out))

  (defn render-module [portrait]
    "Render a module portrait as text."
    (def @out @"")

    (when (not (empty? (get portrait :pure)))
      (push out
            (string/format "  Pure:         {}\n"
                           (string/join (->list (get portrait :pure)) ", "))))

    (when (not (empty? (get portrait :boundaries)))
      (push out "\n  Signal boundaries:\n")
      (each b in (get portrait :boundaries)
        (push out
              (string/format "    {} → {} ({})\n" (get b :caller)
                             (get b :callee) (get b :transition)))))

    (each section in [[:false-mutable "Mutable bindings never reassigned"]
                      [:unused-binding "Bindings never used"]
                      [:non-tail-recursion
                       "Self-recursion outside tail position"]]
      (let [advisories (get portrait (get section 0))]
        (when (not (empty? advisories))
          (push out (string/format "\n  {}:\n" (get section 1)))
          (each m in advisories
            (push out
                  (string/format "    line {}: {}\n" (get m :line)
                                 (get m :message)))))))

    (let [graph (get portrait :graph)]
      (when (not (empty? (get graph :roots)))
        (push out
              (string/format "\n  Roots:        {}\n"
                             (string/join (->list (get graph :roots)) ", "))))
      (when (not (empty? (get graph :leaves)))
        (push out
              (string/format "  Leaves:       {}\n"
                             (string/join (->list (get graph :leaves)) ", ")))))

    (freeze out))

  # ── Export ──────────────────────────────────────────────────────────────

  {:function function-portrait
   :module module-portrait
   :render render-function
   :render-module render-module
   :failures detect-failure-modes
   :composition assess-composition
   :observations find-observations
   :false-mutable false-mutable-advisories
   :unused-binding unused-binding-advisories
   :non-tail-recursion non-tail-recursion-advisories})
# end closure
