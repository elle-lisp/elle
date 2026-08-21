(elle/epoch 12)

# ── CLI argument parsing test suite ──────────────────────────────

(def cli ((import "std/cli")))

# ── Basic flag ───────────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}]}
                   ["app" "-v"])]
  (assert (= r:verbose true) "short flag"))

# ── Long option with value ───────────────────────────────────────

(let [r (cli:parse {:name "app" :args [{:name "output" :long "output"}]}
                   ["app" "--output" "file.txt"])]
  (assert (= r:output "file.txt") "long option value"))

# ── Long option with = syntax ────────────────────────────────────

(let [r (cli:parse {:name "app" :args [{:name "output" :long "output"}]}
                   ["app" "--output=file.txt"])]
  (assert (= r:output "file.txt") "long option = syntax"))

# ── Default value ────────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "port" :long "port" :default "8080"}]} ["app"])]
  (assert (= r:port "8080") "default value"))

# ── Count action ─────────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :count}]}
                   ["app" "-vvv"])]
  (assert (= r:verbose 3) "stacked count"))

# ── Append action ────────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "include" :long "include" :action :append}]}
                   ["app" "--include" "a" "--include" "b"])]
  (assert (= (length r:include) 2) "append count")
  (assert (= (r:include 0) "a") "append first")
  (assert (= (r:include 1) "b") "append second"))

# ── Positional argument ─────────────────────────────────────────

(let [r (cli:parse {:name "app" :args [{:name "file"}]} ["app" "input.txt"])]
  (assert (= r:file "input.txt") "positional"))

# ── Mixed flags and positionals ──────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}
                           {:name "output" :long "output"} {:name "file"}]}
                   ["app" "-v" "--output" "out.txt" "in.txt"])]
  (assert (= r:verbose true) "mixed: flag")
  (assert (= r:output "out.txt") "mixed: option")
  (assert (= r:file "in.txt") "mixed: positional"))

# ── Required arg missing ─────────────────────────────────────────

(let [[ok _] (protect (cli:parse {:name "app"
                                  :args [{:name "file" :required true}]} ["app"]))]
  (assert (not ok) "required missing errors"))

# ── Unknown option errors ────────────────────────────────────────

(let [[ok _] (protect (cli:parse {:name "app" :args []} ["app" "--bogus"]))]
  (assert (not ok) "unknown long option errors"))

(let [[ok _] (protect (cli:parse {:name "app" :args []} ["app" "-x"]))]
  (assert (not ok) "unknown short option errors"))

# ── Subcommands ──────────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}]
                    :commands [{:name "build"
                                :args [{:name "target" :long "target"}]}]}
                   ["app" "-v" "build" "--target" "release"])]
  (assert (= r:verbose true) "subcommand: parent flag")
  (assert (= r:command "build") "subcommand: name")
  (assert (= r:command-args:target "release") "subcommand: child option"))

# ── Empty argv ───────────────────────────────────────────────────

(let [r (cli:parse {:name "app" :args []} ["app"])]
  (assert (struct? r) "empty argv returns struct"))

# ── -- separator stops option parsing ──────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}
                           {:name "file"}]} ["app" "--" "-v"])]
  (assert (= r:verbose false) "-- separator: flag not parsed")
  (assert (= r:file "-v") "-- separator: -v treated as positional"))

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}
                           {:name "a"} {:name "b"}]} ["app" "-v" "--" "x" "y"])]
  (assert (= r:verbose true) "-- separator: flag before -- still works")
  (assert (= r:a "x") "-- separator: first positional after --")
  (assert (= r:b "y") "-- separator: second positional after --"))

# ── :rest action collects remaining positionals ────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "cmd"} {:name "files" :action :rest}]}
                   ["app" "build" "a.txt" "b.txt" "c.txt"])]
  (assert (= r:cmd "build") "rest: first positional")
  (assert (= (length r:files) 3) "rest: collects remaining count")
  (assert (= (r:files 0) "a.txt") "rest: first")
  (assert (= (r:files 1) "b.txt") "rest: second")
  (assert (= (r:files 2) "c.txt") "rest: third"))

(let [r (cli:parse {:name "app" :args [{:name "files" :action :rest}]} ["app"])]
  (assert (= (length r:files) 0) "rest: empty when no args"))

(let [r (cli:parse {:name "app"
                    :args [{:name "verbose" :short "v" :action :flag}
                           {:name "files" :action :rest}]}
                   ["app" "-v" "--" "--weird" "-x"])]
  (assert (= r:verbose true) "rest+--: flag before --")
  (assert (= (length r:files) 2) "rest+--: collects after --")
  (assert (= (r:files 0) "--weird") "rest+--: first")
  (assert (= (r:files 1) "-x") "rest+--: second"))

# ── :choices validation ────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "format"
                            :long "format"
                            :choices ["json" "text"]}]}
                   ["app" "--format" "json"])]
  (assert (= r:format "json") "choices: valid value accepted"))

(let [[ok err] (protect (cli:parse {:name "app"
                                    :args [{:name "format"
                                    :long "format"
                                    :choices ["json" "text"]}]}
                                   ["app" "--format" "xml"]))]
  (assert (not ok) "choices: invalid value rejected")
  (assert (= err:reason :invalid-choice) "choices: error reason"))

(let [r (cli:parse {:name "app"
                    :args [{:name "tags"
                            :long "tag"
                            :action :append
                            :choices ["a" "b" "c"]}]}
                   ["app" "--tag" "a" "--tag" "b"])]
  (assert (= (length r:tags) 2) "choices+append: valid values accepted"))

(let [[ok _] (protect (cli:parse {:name "app"
                                  :args [{:name "tags"
                                  :long "tag"
                                  :action :append
                                  :choices ["a" "b"]}]}
                                 ["app" "--tag" "a" "--tag" "x"]))]
  (assert (not ok) "choices+append: invalid value rejected"))

# ── negatable flags ────────────────────────────────────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "color"
                            :long "color"
                            :action :flag
                            :default true
                            :negatable true}]} ["app" "--no-color"])]
  (assert (= r:color false) "negatable: --no-color sets false"))

(let [r (cli:parse {:name "app"
                    :args [{:name "color"
                            :long "color"
                            :action :flag
                            :default true
                            :negatable true}]} ["app" "--color"])]
  (assert (= r:color true) "negatable: --color sets true"))

(let [r (cli:parse {:name "app"
                    :args [{:name "color"
                            :long "color"
                            :action :flag
                            :default true
                            :negatable true}]} ["app"])]
  (assert (= r:color true) "negatable: default true preserved"))

(let [[ok _] (protect (cli:parse {:name "app"
                                  :args [{:name "color"
                                  :long "color"
                                  :action :flag}]} ["app" "--no-color"]))]
  (assert (not ok) "negatable: --no-X fails when not negatable"))

# ── cli:help returns formatted string ─────────────────────────

(let [h (cli:help {:name "myapp"
                   :description "A tool"
                   :version "1.0.0"
                   :args [{:name "verbose"
                           :short "v"
                           :long "verbose"
                           :action :flag
                           :help "Be verbose"}
                          {:name "output"
                           :short "o"
                           :long "output"
                           :help "Output file"
                           :meta "FILE"}
                          {:name "format"
                           :long "format"
                           :choices ["json" "text"]
                           :help "Output format"}
                          {:name "color"
                           :long "color"
                           :action :flag
                           :default true
                           :negatable true
                           :help "Colorize output"}
                          {:name "file" :help "Input file"}]})]
  (assert (string/contains? h "myapp") "help: contains app name")
  (assert (string/contains? h "v1.0.0") "help: contains version")
  (assert (string/contains? h "A tool") "help: contains description")
  (assert (string/contains? h "Usage:") "help: contains usage line")
  (assert (string/contains? h "Options:") "help: contains options header")
  (assert (string/contains? h "-v, --verbose") "help: short+long flag")
  (assert (string/contains? h "--format=json|text") "help: choices as metavar")
  (assert (string/contains? h "--[no-]color") "help: negatable flag syntax")
  (assert (string/contains? h "<file>") "help: positional in usage")
  (assert (string/contains? h "FILE") "help: metavar shown"))

(let [h (cli:help {:name "app"
                   :args [{:name "files" :action :rest :help "Input files"}]})]
  (assert (string/contains? h "<files...>") "help: rest shown as <name...>"))

(let [h (cli:help {:name "app"
                   :commands [{:name "build" :description "Build the project"}
                              {:name "test" :description "Run tests"}]})]
  (assert (string/contains? h "Commands:") "help: commands section")
  (assert (string/contains? h "build") "help: command name")
  (assert (string/contains? h "Build the project") "help: command description"))

# ── auto-help signals :help-requested ──────────────────────────

(let [[ok err] (protect (cli:parse {:name "app"
                                    :args [{:name "verbose"
                                    :short "v"
                                    :action :flag}]} ["app" "--help"]))]
  (assert (not ok) "auto-help: --help signals error")
  (assert (= err:reason :help-requested) "auto-help: reason is :help-requested")
  (assert (string/contains? err:message "Usage:")
          "auto-help: message has help text"))

(let [[ok err] (protect (cli:parse {:name "app"
                                    :args [{:name "verbose"
                                    :short "v"
                                    :action :flag}]} ["app" "-h"]))]
  (assert (not ok) "auto-help: -h signals error")
  (assert (= err:reason :help-requested) "auto-help: -h reason"))

# ── auto-help suppressed when spec claims -h/--help ────────────

(let [r (cli:parse {:name "app"
                    :args [{:name "help" :long "help" :action :flag}]}
                   ["app" "--help"])]
  (assert (= r:help true) "auto-help suppressed: --help is user flag"))

(let [r (cli:parse {:name "app" :args [{:name "host" :short "h"}]}
                   ["app" "-h" "localhost"])]
  (assert (= r:host "localhost") "auto-help suppressed: -h is user option"))

# ── subcommand --help shows subcommand help ────────────────────

(let [[ok err] (protect (cli:parse {:name "app"
                                    :commands [{:name "build"
                                    :description "Build things"
                                    :args [{:name "target" :long "target"}]}]}
                                   ["app" "build" "--help"]))]
  (assert (not ok) "subcmd help: signals error")
  (assert (= err:reason :help-requested) "subcmd help: reason")
  (assert (string/contains? err:message "build")
          "subcmd help: contains subcmd name"))

(println "cli: all tests passed")
