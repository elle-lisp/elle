(elle/epoch 9)

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

(println "cli: all tests passed")
