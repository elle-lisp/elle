(elle/epoch 10)
## lib/cli.lisp — CLI argument parsing (pure Elle)
##
## Declarative argument parsing from a spec struct + argv list.
## Supports flags, string options, count, append, rest, positionals,
## defaults, subcommands, short/long forms, = syntax, -- separator,
## choices validation, negatable flags, and auto-help.
##
## Usage:
##   (def cli ((import "std/cli")))
##   (def args (cli:parse {:name "app"
##                           :args [{:name "verbose" :short "v" :action :flag}
##                                  {:name "output" :long "output"}]}
##                          (sys/args)))

(fn []
  (defn require-string [v key ctx]
    "Error if v is non-nil and not a string."
    (when (and v (not (string? v)))
      (error {:error :type-error
              :reason :wrong-type
              :expected :string
              :got (type-of v)
              :param (keyword key)
              :message (string ":" key " must be a string, got " (type-of v))}))
    v)

  ## ── Arg spec parsing ─────────────────────────────────────────────

  (defn parse-arg-spec [spec]
    "Normalize an arg spec struct into internal form."
    (unless (struct? spec)
      (error {:error :cli-error
              :reason :invalid-spec
              :expected :struct
              :got (type-of spec)
              :message "each arg must be a struct"}))
    (let [name (require-string spec:name "name" "cli/parse")]
      (unless name
        (error {:error :cli-error
                :reason :missing-name
                :message "each arg must have a :name key"}))
      (let* [long-name (require-string spec:long "long" "cli/parse")
             short-name (require-string spec:short "short" "cli/parse")
             action-kw (let [v spec:action]
                         (if (nil? v)
                           :set
                           (if (keyword? v)
                             v
                             (error {:error :cli-error
                                     :reason :wrong-type
                                     :expected :keyword
                                     :got (type-of v)
                                     :param :action
                                     :message (string ":action must be a keyword, got "
                                     (type-of v))}))))
             default-val spec:default
             required? spec:required
             help-text (require-string spec:help "help" "cli/parse")
             meta-var (require-string spec:meta "meta" "cli/parse")
             choices spec:choices
             negatable? spec:negatable]
        (when (and short-name (not (= (length short-name) 1)))
          (error {:error :cli-error
                  :reason :invalid-short
                  :option short-name
                  :message (string ":short must be a single character, got \""
                                   short-name "\"")}))
        (unless (contains? |:set :flag :count :append :rest| action-kw)
          (error {:error :cli-error
                  :reason :unknown-action
                  :action action-kw
                  :message (string "unknown action " action-kw
                                   ", expected :set, :flag, :count, :append, or :rest")}))
        (when (and choices (not (array? choices)))
          (error {:error :cli-error
                  :reason :invalid-choices
                  :message ":choices must be an array of strings"}))
        (when (and negatable? (not (= action-kw :flag)))
          (error {:error :cli-error
                  :reason :invalid-negatable
                  :message ":negatable is only valid on :flag actions"}))
        {:name name
         :long long-name
         :short short-name
         :action action-kw
         :default default-val
         :required required?
         :help help-text
         :meta meta-var
         :choices choices
         :negatable negatable?})))

  ## ── Argv parsing engine ──────────────────────────────────────────

  (defn find-by-long [specs long-name]
    (find (fn [s] (= s:long long-name)) specs))

  (defn find-by-short [specs ch]
    (find (fn [s] (= s:short ch)) specs))

  (defn positionals [specs]
    (filter (fn [s] (and (nil? s:long) (nil? s:short))) specs))

  (defn apply-action [result name action value]
    (let [k (keyword name)]
      (match action
        :set (put result k value)
        :flag (put result k true)
        :count
          (put result k (inc (result k)))
        :append
          (begin
            (push (result k) value)
            result)
        :rest
          (begin
            (push (result k) value)
            result)
        _ result)))

  (defn validate-choices [spec value option-str]
    (when (and spec:choices (not (find (fn [c] (= c value)) spec:choices)))
      (error {:error :cli-error
              :reason :invalid-choice
              :option option-str
              :value value
              :choices spec:choices
              :message (string option-str ": expected one of "
                               (string/join spec:choices ", ") "; got \"" value
                               "\"")})))

  (defn init-result [specs]
    (let [r @{}]
      (each s in specs
        (let [k (keyword s:name)]
          (match s:action
            :flag
              (put r k (if (nil? s:default) false s:default))
            :count (put r k 0)
            :append (put r k @[])
            :rest (put r k @[])
            _ (put r k s:default))))
      r))

  (defn handle-positional [result pos-specs pi arg]
    (if (< pi (length pos-specs))
      (let [ps (pos-specs pi)]
        (if (= ps:action :rest)
          (begin
            (push (result (keyword ps:name)) arg)
            pi)
          (begin
            (put result (keyword ps:name) arg)
            (inc pi))))
      (if (and (> (length pos-specs) 0)
               (= ((pos-specs (dec (length pos-specs))) :action) :rest))
        (let [ps (pos-specs (dec (length pos-specs)))]
          (push (result (keyword ps:name)) arg)
          pi)
        (error {:error :cli-error
                :reason :unexpected-argument
                :argument arg
                :message (string "unexpected argument \"" arg "\"")}))))

  (defn parse-argv [specs argv]
    "Parse argv list against normalized specs. Returns mutable struct."
    (let* [result (init-result specs)
           pos-specs (positionals specs)
           args (->array argv)
           argc (length args)]
      (def @pi 0)
      (def @i 0)
      (def @past-sep false)
      (while (< i argc)
        (let [arg (args i)]
          (if past-sep
            (assign pi (handle-positional result pos-specs pi arg))
            (cond
              (= arg "--") (assign past-sep true)  ## --long=value
              (and (string/starts-with? arg "--") (string/contains? arg "="))
                (let* [eq (string/find arg "=")
                       name (slice arg 2 eq)
                       value (slice arg (inc eq) (length arg))
                       spec (find-by-long specs name)]
                  (unless spec
                    (error {:error :cli-error
                            :reason :unknown-option
                            :option (string "--" name)
                            :message (string "unknown option --" name)}))
                  (apply-action result spec:name spec:action value)
                  (validate-choices spec value (string "--" name)))  ## --long
              (string/starts-with? arg "--")
                (let [name (slice arg 2 (length arg))]
                  (if (and (string/starts-with? name "no-")
                           (let [base (slice name 3 (length name))
                                 s (find-by-long specs base)]
                             (and s s:negatable)))
                    (let* [base (slice name 3 (length name))
                           spec (find-by-long specs base)]
                      (put result (keyword spec:name) false))
                    (let [spec (find-by-long specs name)]
                      (unless spec
                        (error {:error :cli-error
                                :reason :unknown-option
                                :option (string "--" name)
                                :message (string "unknown option --" name)}))
                      (match spec:action
                        :flag (apply-action result spec:name :flag nil)
                        :count (apply-action result spec:name :count nil)
                        _
                          (begin
                            (assign i (inc i))
                            (when (>= i argc)
                              (error {:error :cli-error
                                      :reason :missing-value
                                      :option (string "--" name)
                                      :message (string "--" name
                                      " requires a value")}))
                            (apply-action result spec:name spec:action (args i))
                            (validate-choices spec (args i) (string "--" name)))))))
              (and (string/starts-with? arg "-") (> (length arg) 1))
                (let [chars (slice arg 1 (length arg))]
                  (def @ci 0)
                  (while (< ci (length chars))
                    (let* [ch (chars ci)
                           spec (find-by-short specs ch)]
                      (unless spec
                        (error {:error :cli-error
                                :reason :unknown-option
                                :option (string "-" ch)
                                :message (string "unknown option -" ch)}))
                      (match spec:action
                        :flag (apply-action result spec:name :flag nil)
                        :count (apply-action result spec:name :count nil)
                        _
                          (if (< (inc ci) (length chars))
                            (let [val (slice chars (inc ci) (length chars))]
                              (apply-action result spec:name spec:action val)
                              (validate-choices spec val (string "-" ch))
                              (assign ci (length chars)))
                            (begin
                              (assign i (inc i))
                              (when (>= i argc)
                                (error {:error :cli-error
                                        :reason :missing-value
                                        :option (string "-" ch)
                                        :message (string "-" ch
                                        " requires a value")}))
                              (apply-action result spec:name spec:action
                              (args i))
                              (validate-choices spec (args i) (string "-" ch))))))
                    (assign ci (inc ci))))
              true (assign pi (handle-positional result pos-specs pi arg)))))
        (assign i (inc i)))  ## Check required args
      (each s in specs
        (when s:required
          (when (nil? (result (keyword s:name)))
            (error {:error :cli-error
                    :reason :missing-required
                    :name s:name
                    :message (string "missing required argument: " s:name)}))))
      result))

  ## ── Help formatting ────────────────────────────────────────────

  (defn format-opt-left [spec]
    "Build the left column string for one option in help output."
    (if (and (nil? spec:long) (nil? spec:short))
      nil
      (let [short-part (if spec:short (string "-" spec:short) nil)
            long-part (cond
                        (and spec:long spec:negatable) (string "--[no-]"
                        spec:long)
                        spec:long (string "--" spec:long)
                        true nil)
            combined (cond
                       (and short-part long-part) (string short-part ", "
                       long-part)
                       short-part short-part
                       long-part (string "    " long-part)
                       true "")
            suffix (if (contains? |:set :append| spec:action)
                     (let [mv (cond
                                spec:meta spec:meta
                                spec:choices (string/join spec:choices "|")
                                true "VALUE")]
                       (string "=" mv))
                     "")]
        (string "  " combined suffix))))

  (defn format-help [spec]
    "Assemble full help text from a command spec."
    (let* [norm-args (map parse-arg-spec (or spec:args []))
           name (or spec:name "program")
           lines @[]
           header (string name (if spec:version (string " v" spec:version) "")
                          (if spec:description
                            (string " — " spec:description)
                            ""))
           named-args (filter (fn [s] (or s:long s:short)) norm-args)
           pos-args (positionals norm-args)
           cmds (or spec:commands [])]
      (push lines header)
      (push lines "")
      (let [usage-parts @[name]
            _ (when (> (length named-args) 0) (push usage-parts "[OPTIONS]"))
            _ (each p in pos-args
                (if (= p:action :rest)
                  (push usage-parts (string "<" p:name "...>"))
                  (push usage-parts (string "<" p:name ">"))))]
        (push lines (string "Usage: " (string/join (->list usage-parts) " "))))
      (when (> (length named-args) 0)
        (let [left-cols (map format-opt-left named-args)
              max-w (min 28
                         (+ 2 (fold (fn [mx s] (max mx (length s))) 0 left-cols)))]
          (push lines "")
          (push lines "Options:")
          (each idx in (range (length named-args))
            (let* [s (named-args idx)
                   left (left-cols idx)
                   right-parts @[]
                   _ (when s:help (push right-parts s:help))
                   _ (when (and s:default (= s:action :flag))
                       (push right-parts
                             (if s:default "(default: on)" "(default: off)")))
                   _ (when (and s:default (not (= s:action :flag)))
                       (push right-parts (string "(default: \"" s:default "\")")))
                   right (string/join (->list right-parts) " ")
                   pad-len (max 1 (- max-w (length left)))
                   pad (string/repeat " " pad-len)]
              (push lines (string left pad right))))))
      (when (> (length cmds) 0)
        (let* [cmd-lefts (map (fn [c] (string "  " c:name)) cmds)
               max-w (min 28
                          (+ 2
                             (fold (fn [mx s] (max mx (length s))) 0 cmd-lefts)))]
          (push lines "")
          (push lines "Commands:")
          (each idx in (range (length cmds))
            (let* [c (cmds idx)
                   left (cmd-lefts idx)
                   desc (or c:description "")
                   pad-len (max 1 (- max-w (length left)))
                   pad (string/repeat " " pad-len)]
              (push lines (string left pad desc))))))
      (string/join (->list lines) "\n")))

  (defn help [spec]
    "Return formatted help string for a command spec."
    (format-help spec))

  ## ── Auto-help detection ────────────────────────────────────────

  (defn check-auto-help [spec norm-args argv]
    "If argv contains --help/-h and no spec claims those, signal help."
    (let [has-help-long (find (fn [s] (= s:long "help")) norm-args)
          has-help-short (find (fn [s] (= s:short "h")) norm-args)
          argv-arr (->array argv)]
      (when (not (and has-help-long has-help-short))
        (each idx in (range (length argv-arr))
          (let [arg (argv-arr idx)]
            (when (or (and (not has-help-long) (= arg "--help"))
                      (and (not has-help-short) (= arg "-h")))
              (error {:error :cli-error
                      :reason :help-requested
                      :message (format-help spec)})))))))

  ## ── Subcommand support ───────────────────────────────────────────

  (defn parse-with-commands [spec argv]
    "Parse argv, handling subcommands if :commands is present."
    (let* [args-spec (or spec:args [])
           cmds-spec (or spec:commands [])
           norm-args (map parse-arg-spec args-spec)
           has-cmds (> (length cmds-spec) 0)]
      (check-auto-help spec norm-args argv)
      (when has-cmds
        (each s in norm-args
          (when (contains? |"command" "command-args"| s:name)
            (error {:error :cli-error
                    :reason :reserved-name
                    :name s:name
                    :message (string "arg name " s:name
                                     " conflicts with reserved subcommand key")}))))
      (if (not has-cmds)
        (freeze (parse-argv norm-args argv))
        (let* [cmd-names (map (fn [c] (require-string c:name "name" "cli/parse"))
                              cmds-spec)
               result (init-result norm-args)
               args-arr (->array argv)
               argc (length args-arr)]
          (def @i 0)
          (def @found nil)
          (def @cmd-start nil)  ## Scan for subcommand name
          (while (and (< i argc) (nil? found))
            (let [arg (args-arr i)]
              (if (and (not (string/starts-with? arg "-"))
                       (find (fn [n] (= n arg)) cmd-names))
                (begin
                  (assign found arg)
                  (assign cmd-start (inc i)))
                (assign i (inc i)))))  ## Parse parent args (everything before the subcommand)
          (let [parent-argv (->list (slice args-arr 0 i))]
            (each [k v] in (pairs (parse-argv norm-args parent-argv))
              (put result k v)))
          (if (nil? found)
            (begin
              (put result :command nil)
              (put result :command-args nil))
            (begin
              (put result :command found)
              (let* [sub-spec (find (fn [c] (= c:name found)) cmds-spec)
                     sub-argv (->list (slice args-arr cmd-start argc))
                     sub-result (parse-with-commands sub-spec sub-argv)]
                (put result :command-args sub-result))))
          (freeze result)))))

  ## ── Entry point ──────────────────────────────────────────────────

  (defn parse [spec argv]
    "Parse CLI arguments against a command spec. Returns struct of parsed values."
    (unless (struct? spec)
      (error {:error :type-error
              :reason :wrong-type
              :expected :struct
              :got (type-of spec)
              :message (string "spec must be a struct, got " (type-of spec))}))
    (unless (or (array? argv) (pair? argv) (empty? argv))
      (error {:error :type-error
              :reason :wrong-type
              :expected :list
              :got (type-of argv)
              :message (string "argv must be a list or array, got "
                               (type-of argv))}))
    (unless spec:name
      (error {:error :cli-error
              :reason :missing-name
              :message "spec must have a :name key"}))  ## Skip argv[0] (program name)
    (let [user-argv (if (> (length argv) 0) (rest argv) ())]
      (parse-with-commands spec (->list user-argv))))

  {:parse parse :help help})
