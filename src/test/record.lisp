(elle/epoch 12)
# audited: 2026-09-21
## elle test — turning an outcome into rows: the label a form is known by, what
## analysis finds in it, the status a payload classifies to, and one row per
## (form × tier).
## docs/test-store.md
##
## A fragment of one module (see store.lisp).

# ── label: scavenge the first assert message from a form's syntax ────
(defn scan-msg [x]
  (if (list? x)
    (if (and (>= (length x) 3) (= (first x) (quote assert)) (string? (get x 2)))
      (get x 2)
      (scan-children x))
    nil))
(defn scan-children [xs]
  (if (empty? xs)
    nil
    (let [m (scan-msg (first xs))]
      (if m m (scan-children (rest xs))))))

(defn sig-of [payload]
  (let [e (get payload :error)]
    (if e (string ":" (string e)) ":error")))
# render keyword with its leading colon

# Render a payload field to text, or nil when absent (so it lands as SQL NULL).
(defn field-str [payload key]
  (let [v (get payload key)]
    (if (= v nil) nil (string v))))

# ── classify one tier's [ok? payload] into a row ─────────────────────
# pass  — the closure returned a value (held in :value for divergence checking).
# skip  — a loud gate fired (:gated) or the tier rejected this form (:ineligible).
# fail  — any other error; capture the assert payload (:syntax/:actual/:expected).
(defn classify [r]
  (let [ok (get r 0)
        raw (get r 1)  # A failure payload is normally a typed-error struct, but a worker can
        # hand back a bare unsendable object (an io-request, a fiber) as the
        # "error" — coerce anything non-struct so classify never faults the run.
        payload (if (struct? raw)
                  raw
                  (struct :error :error :message (string raw)))]
    (if ok
      (struct :status :pass :ok true :value raw)
      (let [err (get payload :error)]
        (if (= err :gated)
          (struct :status :skip :ok false :reason (field-str payload :reason))  # A trapped (exit): exit 0 is an opt-out (skip), any other code a fail.
          # See sys/trap-exit! — the runner traps exit so a test can't truncate
          # the run; this records what the test asked for.
          (if (= err :exited)
            (if (= (get payload :code) 0)
              (struct :status :skip :ok false :reason "exit 0")
              (struct :status :fail :ok false :sig ":exited"
                      :reason (field-str payload :message)))
            (if (= err :timeout)
              (struct :status :timeout :ok false
                      :reason (field-str payload :message))
              (if (and (= err :tier-rejected)
                       (= (get payload :reason) :ineligible))
                (struct :status :skip :ok false
                        :reason (field-str payload :message))
                (struct :status :fail :ok false :sig (sig-of payload)
                        :reason (field-str payload :message)
                        :syn (field-str payload :syntax)
                        :act (field-str payload :actual)
                        :exp (field-str payload :expected))))))))))

# ── what analysis says about a form (docs/test-store.md) ─────────────
# The function a form's source is analyzed as. A file's top level and a
# function body are both letrec-scoped, so one function whose body is the file
# is the form, and the signal inferred for that function is the form's own
# effect profile. The name is the runner's: a corpus file that defined it
# would have its profile read off the wrong body.
(def profile-probe "elle-test-form-profile")

# The capability bits — the resource classes a verdict depends on. `error` and
# `yield` are signals and not capabilities: a form that raises, and a form that
# suspends, are both still functions of their own inputs.
(def capability-bits |"debug" "exec" "ffi" "fs" "gpu" "io" "os-signal"|)

# The profile of a form the compiler would not analyze. Three NULLs, which say
# the analysis did not answer — never that the form reaches nothing.
(def no-profile (struct :caps nil :touches nil :signal nil))

(defn bit-names [sig]
  "One inferred signal's bits, as sorted names."
  (let [@out @[]]
    (each b in (get sig :bits)
      (push out (string b)))
    (sort out)))

(defn touched-names [a]
  "Every binding the analysis calls, less the ones the source defines itself.
   A form that touched its own helpers would answer a selection by binding
   with the forms that merely named one."
  (let [@own @{}]
    (each s in (compile/symbols a)
      (put own (get s :name) true))
    (let [@out @[]]
      (each n in (get (compile/call-graph a) :nodes)
        (each c in (get n :callees)
          (when (not (get own c)) (push out c))))
      (sort (distinct out)))))

(defn form-profile [src name]
  "What the compiler finds in SRC, as {:caps :touches :signal}, each a
   space-separated list. Every step is protected: a profile is a reading about
   a form, so a form that will not analyze costs the three columns and nothing
   else — the run still compiles it, runs it, and records its verdict."
  (let [[ok? a] (protect (compile/analyze (string "(defn " profile-probe " []\n"
                         src "\n)") {:file name}))]
    (if (not ok?)
      no-profile
      (let [[read? sig] (protect (compile/signal a profile-probe))]
        (if (not read?)
          no-profile
          (let [names (bit-names sig)]
            (struct :caps (string/join (filter (fn [n]
                                         (contains? capability-bits n)) names)
                                       " ")
                    :touches (string/join (touched-names a) " ")
                    :signal (string/join names " "))))))))

# ── the form row ─────────────────────────────────────────────────────
# The row's own fields as one value: what identifies a form, what it is known
# by, and what analysis found in it. One constructor, because a call site that
# wrote the struct out could leave the profile off and nothing would say so —
# the row would simply record NULL, which is the answer reserved for an
# analysis that refused.
(defn form-row [h origin file idx label src profile]
  (struct :hash h :origin (string origin) :file (string file) :index idx
          :label label :src src :caps (get profile :caps)
          :touches (get profile :touches) :signal (get profile :signal)))

(defn form-row-of [origin file idx label src profile]
  "The row for a form identified by the hash of its own syntax."
  (form-row (string (hash src)) origin file idx label src profile))

(defn file-row [kind origin file label src profile]
  "The row for a file that produced no test form — a compile error, or a gated
   shared setup. KIND separates the two synthetic hashes."
  (form-row (string (hash (string kind ":" file))) origin file -1 label src
            profile))

# Insert the `form` row every recording path needs first. A form is deduped
# across runs by the hash of its syntax, so a second run of the same code
# re-uses the row rather than adding one — which is what lets a result join to
# a form and a form's history join across runs. IGNORE, not REPLACE: the row
# that is already there was written from the same hash.
(defn insert-form [conn row]
  (sqlite:exec conn
               "INSERT OR IGNORE INTO form (hash, origin, file, form_index, label, src, caps, touches, signal) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
               [(get row :hash) (get row :origin) (get row :file)
                (get row :index) (get row :label) (get row :src) (get row :caps)
                (get row :touches) (get row :signal)]))

# Insert one (form × tier) result row and return its rowid (so assets can
# reference it).
(defn insert-result [conn run-id h tier-str c]
  (sqlite:exec conn
               "INSERT INTO result (run_id, form_hash, tier, status, reason, signal, syntax, expected, actual) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
               [run-id h tier-str (get c :status) (get c :reason) (get c :sig)
                (get c :syn) (get c :exp) (get c :act)])
  (get (get (sqlite:query conn "SELECT last_insert_rowid() AS id") 0) :id))

# Run a form on every active tier, inserting one row per tier and attaching the
# file's captured `dumps` (a list of [kind addr size codec]) as assets to each.
# `exec-fn` is (fn [tier-keyword out-path err-path] -> {:result :stdout :stderr});
# the per-form path closes over a MAIN-compiled thunk (exec-thunk-capture), the
# whole-file path closes over the file's syntax (exec-source-capture). Returns
# [statuses pass-pairs]: the per-tier status strings, and [[tier-str value]...]
# for the tiers that returned a value (divergence candidates).
(defn run-tiers [conn run-id h exec-fn tiers dumps statuses pass-pairs]
  (if (empty? tiers)
    [statuses pass-pairs]
    (let [tp (first tiers)
          tk (get tp 0)
          ts (get tp 1)
          base (string scratch-dir "/" run-id "_" h "_" ts)
          cap (exec-fn tk (string base ".out") (string base ".err"))
          c (note-timeout-stacks (note-last-output (classify (get cap :result))
                                 cap))
          rid (insert-result conn run-id h ts c)]
      (insert-assets conn rid dumps)
      (capture-stdio conn rid (get cap :stdout) (get cap :stderr))
      (run-tiers conn run-id h exec-fn (rest tiers) dumps
                 (concat statuses [(get c :status)])
                 (if (get c :ok)
                   (concat pass-pairs [[ts (get c :value)]])
                   pass-pairs)))))

# Are all of these values equal to the first? (Divergence = NOT all equal.)
(defn all-equal? [x xs]
  (if (empty? xs)
    true
    (and (= x (first xs)) (all-equal? x (rest xs)))))

# Render the passing tiers' values for the synthetic diverge row's reason.
(defn render-pairs [pairs]
  (if (empty? pairs)
    ""
    (let [p (first pairs)
          one (string (get p 0) "=" (string (get p 1)))]
      (if (empty? (rest pairs))
        one
        (string one " " (render-pairs (rest pairs)))))))

# Record ONE thunk under the form identity ROW carries: insert its `form` row
# and run it across every active tier (divergence appended as a synthetic
# tier='*' row). Shared by the per-form path (record-form-result) and the
# whole-file path (the legacy multi-form mode runs one thunk for the file).
# `diverge?` enables the synthetic tier='*' divergence row. The per-form path
# (record-form-result) forces ONE form onto each backend, so distinct values are
# a real cross-tier disagreement — diverge? true. The whole-file path runs an
# imperative SCRIPT under each JIT policy, whose side effects (pids, timestamps)
# differ run-to-run, so a value difference is NOT a bug — diverge? false.
(defn record-thunk [conn run-id row exec-fn dumps tiers diverge?]
  (let [h (get row :hash)]
    (insert-form conn row)
    (let [tr (run-tiers conn run-id h exec-fn tiers dumps [] [])
          statuses (get tr 0)
          pass-pairs (get tr 1)
          vals (map (fn [pp] (get pp 1)) pass-pairs)]
      (if (and diverge? (> (length pass-pairs) 1)
               (not (all-equal? (first vals) (rest vals))))
        (begin
          (sqlite:exec conn
                       "INSERT INTO result (run_id, form_hash, tier, status, reason) VALUES (?1,?2,?3,?4,?5)"
                       [run-id h (keyword "*") :diverge
                        (render-pairs pass-pairs)])
          (concat statuses [:diverge]))
        statuses))))

(defn record-form-result [conn run-id row thunk dumps]
  (record-thunk conn run-id row (fn [tk o e] (exec-thunk-capture tk thunk o e))
                dumps active-tiers true))

# Iterate the [idx thunk] entries from compile/barrier-module. `forms` is the
# file's source forms (unevaluated, epoch-dropped) indexed the same way, so
# entry idx → forms[idx] supplies each test form's label/hash/src. def/var
# setup forms produce no entry (they ran eagerly during the compile pass).
# `profile` is the file's, which for the one-form-per-file corpus shape is the
# form's own (docs/test-store.md § What analysis says about a form).
(defn process-entries [conn run-id origin file forms profile entries dumps acc]
  (if (empty? entries)
    acc
    (let [e (first entries)
          idx (get e 0)
          thunk (get e 1)
          form (get forms idx)
          msg (scan-msg form)
          row (form-row-of origin file idx (if msg msg "") (string form) profile)
          statuses (record-form-result conn run-id row thunk dumps)]
      (process-entries conn run-id origin file forms profile (rest entries)
                       dumps (concat acc statuses)))))

# A file that won't compile (or whose setup faults) has no test forms to run:
# record ONE file-level failure (a `vm` row joined to a synthetic form row whose
# `file` is the offending file, so SQL selection by file still finds it).
(defn record-file-error [conn run-id origin file payload dumps profile]
  (let [msg (field-str payload :message)
        row (file-row "file-error" origin file "file-level error"
                      (if msg msg "") profile)
        h (get row :hash)]
    (insert-form conn row)
    (sqlite:exec conn
                 "INSERT INTO result (run_id, form_hash, tier, status, reason, signal) VALUES (?1,?2,?3,?4,?5,?6)"
                 [run-id h :vm :fail msg (sig-of payload)])  # Attach whatever artifacts compiled (a non-compiling file often still
    # parses to an `ast`), so even a file-level failure has a queryable record.
    (insert-assets conn
                   (get (get (sqlite:query conn
                             "SELECT last_insert_rowid() AS id") 0) :id) dumps)
    [:fail]))

# A file whose eager SHARED SETUP raised a loud `(gate! …)` (`:gated`) — e.g. an
# optional FFI library that wouldn't load, re-raised as :gated at its import
# site. The compile aborts before any test thunk is built, so there are no
# per-form results to record; we mirror record-file-error but as a SKIP (the
# dependency is absent, not broken). One file-level row (form_index -1), counted
# in n_skip, leaves the gate exit at 0. See docs/test-runner.md § Gating.
(defn record-file-gated [conn run-id origin file payload dumps profile]
  (let [reason (field-str payload :reason)
        row (file-row "file-gated" origin file "file-level gated"
                      (if reason reason "") profile)
        h (get row :hash)]
    (insert-form conn row)
    (sqlite:exec conn
                 "INSERT INTO result (run_id, form_hash, tier, status, reason, signal) VALUES (?1,?2,?3,?4,?5,?6)"
                 [run-id h :vm :skip reason ":gated"])
    (insert-assets conn
                   (get (get (sqlite:query conn
                             "SELECT last_insert_rowid() AS id") 0) :id) dumps)
    [:skip]))

# The (elle/epoch N) declaration is file metadata, not a test — drop it.
(defn epoch-form? [f]
  (and (list? f) (> (length f) 0) (= (first f) (quote elle/epoch))))

(defn test-forms [src]
  (filter (fn [f] (not (epoch-form? f))) (read-all src)))

# Interpret a compile/{barrier,whole}-module result: ENTRIES (the [idx thunk]
# accumulator) on success → RUN-FN; a `:gated` shared-setup → one file-level
# SKIP; any other setup/compile fault → one file-level FAIL.
(defn dispatch-compiled [conn run-id origin file out dumps profile run-fn]
  (if (get out 0)
    (run-fn (get out 1))
    (if (= (get (get out 1) :error) :gated)
      (record-file-gated conn run-id origin file (get out 1) dumps profile)
      (record-file-error conn run-id origin file (get out 1) dumps profile))))

# A legacy multi-form file is one imperative script: compile it as a single
# whole-file thunk (compile/whole-module) and run that ONE thunk per tier, in
# source order, in isolation — matching a direct run. The per-form barrier (which
# hoists def/var eagerly ahead of the bare-expression test forms) reorders such a
# script (read-before-write) and re-runs shared mutations per tier; one thunk
# eliminates that. The file is its own form: src = the file, label = the first
# assert message anywhere in it. See docs/test-runner.md § Multi-form files.
(defn process-whole [conn run-id origin file name src forms profile dumps]  # Compile ONCE in the main VM to detect a compile error or a top-level :gated
  # (dispatch-compiled records the file-level error/skip row) — but DON'T run that
  # thunk. For execution we ship the file's parsed SYNTAX to a worker that
  # compiles + runs it with its own stdlib (exec-source-capture), so a file whose
  # forms `import` a yielding module (sync/redis/http2/process/grpc/subprocess)
  # shares one scheduler with the worker's ev/run. read-forms is sendable syntax.
  (let [out (protect (compile/whole-module src name))]
    (dispatch-compiled conn run-id origin file out dumps profile
                       (fn [entries]
                         (let [msg (scan-children forms)
                               read-forms (compile/read-forms src name)]
                           (record-thunk conn run-id
                           (form-row-of origin file 0 (if msg msg "") src
                                        profile)
                           (fn [tk o e]
                             (exec-source-capture tk read-forms name o e)) dumps
                           whole-file-policies false))))))

# Compile SRC and run its test forms per tier. A single-form file/snippet (the
# durable corpus shape) uses the per-form barrier (compile/barrier-module); a
# legacy MULTI-form file is wrapped as one whole-file thunk (process-whole). A
# compile/setup error becomes one file-level failure.
(defn process-source [conn run-id origin file name src]
  (let [forms (test-forms src)
        dumps (capture-dumps src name)
        profile (form-profile src name)]
    (if (> (length forms) 1)
      (process-whole conn run-id origin file name src forms profile dumps)
      (dispatch-compiled conn run-id origin file
                         (protect (compile/barrier-module src name)) dumps
                         profile
                         (fn [entries]
                           (process-entries conn run-id origin file forms
                           profile entries dumps []))))))

(defn process-file [conn run-id file]
  (process-source conn run-id file file file (slurp file)))

# Run FILE as its own process under FLAGS and record what the child left
# behind. The file is the unit here — a process cannot be given one form of it
# — and it is identified by the hash of its source, the same identity the
# whole-file path uses. So an isolated result and an in-process one are two
# tiers of one form rather than two forms that never meet in a query.
# See docs/test-runner.md § Isolation.
(defn process-file-isolated [conn run-id file flags]
  (let [[read-ok? src] (protect (slurp file))]
    (if (not read-ok?)
      (record-file-error conn run-id file file src [] no-profile)
      (let [[parse-ok? forms] (protect (test-forms src))
            msg (if parse-ok? (scan-children forms) nil)
            row (form-row-of file file 0 (if msg msg "") src
                             (form-profile src file))
            h (get row :hash)
            sink (measurement-sink run-id h)
            cap (run-child (child-argv flags file) test-timeout-ms
                           (measurement-env sink))
            c (classify-child cap test-timeout-ms)]
        # The label is scavenged from the source, and a file the child will
        # reject as unreadable has none to give — the child's own status is
        # the verdict either way, so a failed scan costs the label and nothing
        # else. The profile is read here rather than in the child: a form's
        # effect profile is a property of its source, and this process has the
        # compiler open already.
        (insert-form conn row)
        (let [rid (insert-result conn run-id h :process c)]
          (capture-stdio conn rid (get cap :stdout) (get cap :stderr))
          # A dashboard reports its verdicts through the channel named in the
          # child's environment; every other file writes nothing there.
          (record-measurements conn run-id rid sink))
        [(get c :status)]))))

(defn process-eval [conn run-id expr]
  (process-source conn run-id ":adhoc" "<eval>" "<eval>" expr))
