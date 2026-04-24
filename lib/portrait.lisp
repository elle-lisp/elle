(elle/epoch 9)
## lib/portrait.lisp — semantic portraits from compile/analyze
##
## Builds structured descriptions of functions and modules from analysis
## handles, surfacing non-obvious properties: effect phases, failure
## modes, composition properties, and observations about implicit
## decisions.
##
## Usage:
##   (def portrait (import "std/portrait"))
##   (def a (compile/analyze (file/read "myfile.lisp") {:file "myfile.lisp"}))
##   (println (portrait:function a :my-fn))
##   (println (portrait:module a))
##   (println (portrait:render (portrait:function a :my-fn)))

(fn []

# ── Helpers ─────────────────────────────────────────────────────────────

(defn ->list [coll]
  "Convert any collection to a list for string/join."
  (def @result ())
  (each x in coll
    (assign result (cons x result)))
  result)

# ── Phase classification ────────────────────────────────────────────────

(defn classify-phase [sig]
  "Classify a signal into a phase kind."
  (cond
    (get sig :io)                         :io
    (not (empty? (get sig :propagates)))  :delegated
    (get sig :yields)                     :suspending
    true                                  :pure))

(defn classify-phases [analysis callees]
  "Group callees into sequential phases by effect type."
  (def @current-kind nil)
  (def @current-fns @[])
  (def @phases @[])

  (each callee in callees
    (let* [name (get callee :name)
           sig-result (protect (compile/signal analysis (keyword name)))]
      (let [kind (if (get sig-result 0)
                    (classify-phase (get sig-result 1))
                    :pure)]
        (when (not (= kind current-kind))
          (when (not (empty? current-fns))
            (push phases {:kind current-kind :functions (freeze current-fns)}))
          (assign current-kind kind)
          (assign current-fns @[]))
        (push current-fns name))))

  (when (not (empty? current-fns))
    (push phases {:kind current-kind :functions (freeze current-fns)}))

  (freeze phases))

# ── Failure mode detection ──────────────────────────────────────────────

(defn detect-failure-modes [analysis callees]
  "Identify how a function can fail."
  (def @modes @[])

  (each callee in callees
    (let* [name (get callee :name)
           sig-result (protect (compile/signal analysis (keyword name)))]
      (when (get sig-result 0)
        (let [sig (get sig-result 1)]
          (when (contains? (get sig :bits) :error)
            (push modes {:source name
                         :line (get callee :line)
                         :kind :error}))))))

  (freeze modes))

# ── Composition assessment ──────────────────────────────────────────────

(defn assess-composition [sig captures]
  "Determine composition properties from signal and captures."
  (let* [has-mutable-capture (not (empty? (filter (fn [c] (get c :mutated))
                                                   captures)))
         has-io (get sig :io)
         has-any-capture (not (empty? captures))]
    {:retry-safe       (not has-io)
     :timeout-safe     (not has-mutable-capture)
     :parallelizable   (not has-mutable-capture)
     :memoizable       (and (get sig :silent) (not has-any-capture))
     :jit-eligible     (get sig :jit-eligible)
     :stateless        (empty? captures)}))

# ── Observation engine ──────────────────────────────────────────────────

(defn find-observations [analysis name sig captures callees]
  "Generate observations about non-obvious properties."
  (def @obs @[])

  # 1. Almost-pure: only one I/O callee
  (let [io-callees (filter
                      (fn [c]
                        (let [r (protect (compile/signal analysis
                                           (keyword (get c :name))))]
                          (and (get r 0) (get (get r 1) :io))))
                      callees)]
    (when (and (not (get sig :silent))
               (= 1 (length io-callees)))
      (let [io-callee (first io-callees)]
        (push obs {:kind :almost-pure
                   :message (string/format
                     "Only I/O source is {} at line {}. Factoring it out makes the rest JIT-eligible."
                     (get io-callee :name) (get io-callee :line))}))))

  # 2. Mutable capture shared across closures
  (each cap in captures
    (when (get cap :mutated)
      (let [captured-by (protect (compile/captured-by analysis
                                    (keyword (get cap :name))))]
        (when (and (get captured-by 0)
                   (> (length (get captured-by 1)) 1))
          (push obs {:kind :shared-mutable
                     :message (string/format
                       "Mutable binding '{}' is captured by {} functions. Concurrent fibers will race."
                       (get cap :name) (length (get captured-by 1)))})))))

  # 3. Unsandboxed delegation
  (when (not (empty? (get sig :propagates)))
    (each idx in (get sig :propagates)
      (push obs {:kind :unsandboxed-delegation
                 :message (string/format
                   "Parameter {} is called without signal bounds. A malicious closure could do arbitrary I/O, yield indefinitely, or crash the caller. Consider (silence param) or a fuel budget."
                   idx)})))

  # 4. All-tail-call chain
  (when (and (not (empty? callees))
             (not (empty? (filter (fn [c] (get c :tail)) callees)))
             (= (length (filter (fn [c] (get c :tail)) callees))
                (length callees)))
    (push obs {:kind :tail-chain
               :message "All calls are in tail position. This function is a state machine candidate."}))

  # 5. Capture-by-value of mutable source
  (each cap in captures
    (when (and (= (get cap :kind) :value)
               (not (get cap :mutated)))
      (let [binding-result (protect (compile/binding analysis
                                       (keyword (get cap :name))))]
        (when (and (get binding-result 0)
                   (get (get binding-result 1) :mutated))
          (push obs {:kind :stale-capture
                     :message (string/format
                       "Captures '{}' by value, but '{}' is mutated elsewhere. This closure sees the value at capture time, not later mutations."
                       (get cap :name) (get cap :name))})))))

  (freeze obs))

# ── Function portrait ──────────────────────────────────────────────────

(defn function-portrait [analysis name]
  "Build a semantic portrait of a named function."
  (let* [sig      (compile/signal analysis name)
         caps     (compile/captures analysis name)
         callers  (compile/callers analysis name)
         callees  (compile/callees analysis name)
         phases   (classify-phases analysis callees)
         failures (detect-failure-modes analysis callees)
         comp     (assess-composition sig caps)
         obs      (find-observations analysis name sig caps callees)]

    {:name         (string name)
     :signal       sig
     :phases       phases
     :captures     caps
     :failures     failures
     :composition  comp
     :observations obs
     :callers      callers
     :callees      callees}))

# ── Module portrait ─────────────────────────────────────────────────────

(defn module-portrait [analysis]
  "Build a signal topology for an entire module."
  (let* [syms (compile/symbols analysis)
         fns  (filter (fn [s] (= (get s :kind) :function)) syms)]

    (def @pure @[])
    (def @io-boundary @[])
    (def @delegating @[])
    (def @yielding @[])

    (each sym in fns
      (let* [name (get sym :name)
             sig-result (protect (compile/signal analysis (keyword name)))]
        (when (get sig-result 0)
          (let [sig (get sig-result 1)]
            (cond
              (get sig :silent)                         (push pure name)
              (not (empty? (get sig :propagates)))      (push delegating name)
              (get sig :io)                             (push io-boundary name)
              (get sig :yields)                         (push yielding name)
              true                                      (push io-boundary name))))))

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
                (let [callee-result (protect (compile/signal analysis
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

    {:pure        (freeze pure)
     :io-boundary (freeze io-boundary)
     :delegating  (freeze delegating)
     :yielding    (freeze yielding)
     :boundaries  (freeze boundaries)
     :graph       (compile/call-graph analysis)}))

# ── Text rendering ──────────────────────────────────────────────────────

(defn format-signal [sig]
  "Format a signal struct as a readable string."
  (let [bits (get sig :bits)]
    (if (empty? bits)
      (if (empty? (get sig :propagates))
        "silent"
        (string/format "propagates params {}" (get sig :propagates)))
      (string/join (map string (->list bits)) ", "))))

(defn format-phases [phases]
  "Format phase list as a readable string."
  (if (empty? phases)
    "(none)"
    (string/join
      (map (fn [p]
             (string/format "[{}: {}]"
               (get p :kind)
               (string/join (->list (get p :functions)) " ")))
           phases)
      " → ")))

(defn render-function [portrait]
  "Render a function portrait as text."
  (def @out @"")
  (let [name (get portrait :name)
        sig  (get portrait :signal)
        caps (get portrait :captures)
        comp (get portrait :composition)]

    (push out (string/format "{}\n\n" name))
    (push out (string/format "  Effects:       {}\n" (format-signal sig)))
    (push out (string/format "  Phases:        {}\n" (format-phases (get portrait :phases))))

    (when (not (empty? caps))
      (push out (string/format "  Captures:      {}\n"
        (string/join
          (map (fn [c] (string/format "{} ({})" (get c :name) (get c :kind)))
               caps)
          ", "))))

    (when (empty? caps)
      (push out "  Captures:      none\n"))

    (when (not (empty? (get portrait :failures)))
      (push out "\n  Failure modes:\n")
      (each f in (get portrait :failures)
        (push out (string/format "    - {} at line {} ({})\n"
                   (get f :source) (get f :line) (get f :kind)))))

    (push out "\n  Composition:\n")
    (each [k v] in (pairs comp)
      (push out (string/format "    {}: {}\n" k v)))

    (when (not (empty? (get portrait :observations)))
      (push out "\n  Observations:\n")
      (each o in (get portrait :observations)
        (push out (string/format "    [{}] {}\n"
                   (get o :kind) (get o :message))))))

  (freeze out))

(defn render-module [portrait]
  "Render a module portrait as text."
  (def @out @"")

  (when (not (empty? (get portrait :pure)))
    (push out (string/format "  Pure:         {}\n"
      (string/join (->list (get portrait :pure)) ", "))))

  (when (not (empty? (get portrait :io-boundary)))
    (push out (string/format "  I/O:          {}\n"
      (string/join (->list (get portrait :io-boundary)) ", "))))

  (when (not (empty? (get portrait :delegating)))
    (push out (string/format "  Delegating:   {}\n"
      (string/join (->list (get portrait :delegating)) ", "))))

  (when (not (empty? (get portrait :yielding)))
    (push out (string/format "  Yielding:     {}\n"
      (string/join (->list (get portrait :yielding)) ", "))))

  (when (not (empty? (get portrait :boundaries)))
    (push out "\n  Signal boundaries:\n")
    (each b in (get portrait :boundaries)
      (push out (string/format "    {} → {} ({})\n"
                 (get b :caller) (get b :callee) (get b :transition)))))

  (let [graph (get portrait :graph)]
    (when (not (empty? (get graph :roots)))
      (push out (string/format "\n  Roots:        {}\n"
        (string/join (->list (get graph :roots)) ", "))))
    (when (not (empty? (get graph :leaves)))
      (push out (string/format "  Leaves:       {}\n"
        (string/join (->list (get graph :leaves)) ", ")))))

  (freeze out))

# ── Export ──────────────────────────────────────────────────────────────

{:function      function-portrait
 :module        module-portrait
 :render        render-function
 :render-module render-module
 :phases        classify-phases
 :failures      detect-failure-modes
 :composition   assess-composition
 :observations  find-observations})  # end closure
