## lib/cli.lisp — CLI argument parsing (pure Elle)
##
## Declarative argument parsing from a spec struct + argv list.
## Supports flags, string options, count, append, positionals,
## defaults, subcommands, short/long forms, and = syntax.
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
              :message (string ctx ": :" key " must be a string, got " (type-of v))}))
    v)

  ## ── Arg spec parsing ─────────────────────────────────────────────

  (defn parse-arg-spec [spec]
    "Normalize an arg spec struct into internal form."
    (unless (struct? spec)
      (error {:error :cli-error :message "cli/parse: each arg must be a struct"}))
    (let [[name (require-string spec:name "name" "cli/parse")]]
      (unless name
        (error {:error :cli-error :message "cli/parse: each arg must have a :name key"}))
      (let* [[long-name  (require-string spec:long "long" "cli/parse")]
             [short-name (require-string spec:short "short" "cli/parse")]
             [action-kw  (let [[v spec:action]]
                           (if (nil? v) :set
                             (if (keyword? v) v
                               (error {:error :cli-error
                                       :message (string "cli/parse: :action must be a keyword, got "
                                                        (type-of v))}))))]
             [default-val (require-string spec:default "default" "cli/parse")]
             [required?   spec:required]]
        (when (and short-name (not (= (length short-name) 1)))
          (error {:error :cli-error
                  :message (string "cli/parse: :short must be a single character, got \"" short-name "\"")}))
        (unless (contains? |:set :flag :count :append| action-kw)
          (error {:error :cli-error
                  :message (string "cli/parse: unknown action " action-kw
                                   ", expected :set, :flag, :count, or :append")}))
        {:name name :long long-name :short short-name
         :action action-kw :default default-val :required required?})))

  ## ── Argv parsing engine ──────────────────────────────────────────

  (defn find-by-long [specs long-name]
    (find (fn [s] (= s:long long-name)) specs))

  (defn find-by-short [specs ch]
    (find (fn [s] (= s:short ch)) specs))

  (defn positionals [specs]
    (filter (fn [s] (and (nil? s:long) (nil? s:short))) specs))

  (defn apply-action [result name action value]
    (let [[k (keyword name)]]
      (match action
        [:set    (put result k value)]
        [:flag   (put result k true)]
        [:count  (put result k (inc (result k)))]
        [:append (push (result k) value) result]
        [_       result])))

  (defn init-result [specs]
    (let [[r @{}]]
      (each s in specs
        (let [[k (keyword s:name)]]
          (match s:action
            [:flag   (put r k false)]
            [:count  (put r k 0)]
            [:append (put r k @[])]
            [_       (put r k s:default)])))
      r))

  (defn parse-argv [specs argv]
    "Parse argv list against normalized specs. Returns mutable struct."
    (let* [[result    (init-result specs)]
           [pos-specs (positionals specs)]
           [args      (->array argv)]
           [argc      (length args)]]
      (var pi 0)
      (var i 0)
      (while (< i argc)
        (let [[arg (args i)]]
          (cond
            ## --long=value
            ((and (string/starts-with? arg "--") (string/contains? arg "="))
             (let* [[eq    (string/find arg "=")]
                    [name  (slice arg 2 eq)]
                    [value (slice arg (inc eq) (length arg))]
                    [spec  (find-by-long specs name)]]
               (unless spec
                 (error {:error :cli-error :message (string "cli/parse: unknown option --" name)}))
               (apply-action result spec:name spec:action value)))
            ## --long
            ((string/starts-with? arg "--")
             (let* [[name (slice arg 2 (length arg))]
                    [spec (find-by-long specs name)]]
               (unless spec
                 (error {:error :cli-error :message (string "cli/parse: unknown option --" name)}))
               (match spec:action
                 [:flag  (apply-action result spec:name :flag nil)]
                 [:count (apply-action result spec:name :count nil)]
                 [_      (assign i (inc i))
                         (when (>= i argc)
                           (error {:error :cli-error
                                   :message (string "cli/parse: --" name " requires a value")}))
                         (apply-action result spec:name spec:action (args i))])))
            ## -x (short) — handles stacked flags like -vvv
            ((and (string/starts-with? arg "-") (> (length arg) 1))
             (let [[chars (slice arg 1 (length arg))]]
               (var ci 0)
               (while (< ci (length chars))
                 (let* [[ch   (chars ci)]
                        [spec (find-by-short specs ch)]]
                   (unless spec
                     (error {:error :cli-error :message (string "cli/parse: unknown option -" ch)}))
                   (match spec:action
                     [:flag  (apply-action result spec:name :flag nil)]
                     [:count (apply-action result spec:name :count nil)]
                     [_ (if (< (inc ci) (length chars))
                          (begin
                            (apply-action result spec:name spec:action
                                          (slice chars (inc ci) (length chars)))
                            (assign ci (length chars)))
                          (begin
                            (assign i (inc i))
                            (when (>= i argc)
                              (error {:error :cli-error
                                      :message (string "cli/parse: -" ch " requires a value")}))
                            (apply-action result spec:name spec:action (args i))))]))
                 (assign ci (inc ci)))))
            ## Positional
            (true
             (if (< pi (length pos-specs))
               (begin
                 (put result (keyword ((pos-specs pi) :name)) arg)
                 (assign pi (inc pi)))
               (error {:error :cli-error
                       :message (string "cli/parse: unexpected argument \"" arg "\"")})))))
        (assign i (inc i)))
      ## Check required args
      (each s in specs
        (when s:required
          (when (nil? (result (keyword s:name)))
            (error {:error :cli-error
                    :message (string "cli/parse: missing required argument: " s:name)}))))
      result))

  ## ── Subcommand support ───────────────────────────────────────────

  (defn parse-with-commands [spec argv]
    "Parse argv, handling subcommands if :commands is present."
    (let* [[args-spec (or spec:args [])]
           [cmds-spec (or spec:commands [])]
           [norm-args (map parse-arg-spec args-spec)]
           [has-cmds  (> (length cmds-spec) 0)]]
      (when has-cmds
        (each s in norm-args
          (when (contains? |"command" "command-args"| s:name)
            (error {:error :cli-error
                    :message (string "cli/parse: arg name " s:name
                                     " conflicts with reserved subcommand key")}))))
      (if (not has-cmds)
        (freeze (parse-argv norm-args argv))
        (let* [[cmd-names (map (fn [c] (require-string c:name "name" "cli/parse")) cmds-spec)]
               [result    (init-result norm-args)]
               [args-arr  (->array argv)]
               [argc      (length args-arr)]]
          (var i 0)
          (var found nil)
          (var cmd-start nil)
          ## Scan for subcommand name
          (while (and (< i argc) (nil? found))
            (let [[arg (args-arr i)]]
              (if (and (not (string/starts-with? arg "-"))
                       (find (fn [n] (= n arg)) cmd-names))
                (begin (assign found arg) (assign cmd-start (inc i)))
                (assign i (inc i)))))
          ## Parse parent args (everything before the subcommand)
          (let [[parent-argv (->list (slice args-arr 0 i))]]
            (each [k v] in (pairs (parse-argv norm-args parent-argv))
              (put result k v)))
          (if (nil? found)
            (begin (put result :command nil) (put result :command-args nil))
            (begin
              (put result :command found)
              (let* [[sub-spec (find (fn [c] (= c:name found)) cmds-spec)]
                     [sub-argv (->list (slice args-arr cmd-start argc))]
                     [sub-result (parse-with-commands sub-spec sub-argv)]]
                (put result :command-args sub-result))))
          (freeze result)))))

  ## ── Entry point ──────────────────────────────────────────────────

  (defn parse [spec argv]
    "Parse CLI arguments against a command spec. Returns struct of parsed values."
    (unless (struct? spec)
      (error {:error :type-error
              :message (string "cli/parse: spec must be a struct, got " (type-of spec))}))
    (unless (or (array? argv) (pair? argv) (empty? argv))
      (error {:error :type-error
              :message (string "cli/parse: argv must be a list or array, got " (type-of argv))}))
    (unless spec:name
      (error {:error :cli-error :message "cli/parse: spec must have a :name key"}))
    ## Skip argv[0] (program name)
    (let [[user-argv (if (> (length argv) 0) (rest argv) ())]]
      (parse-with-commands spec (->list user-argv))))

  {:parse parse})
